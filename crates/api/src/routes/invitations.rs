use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use buzon_store::app::{Invitation, NewUser, Role};
use buzon_store::{crypto, now, Id};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::Me;
use crate::session::{self, AdminUser};
use crate::{prefs, validate, AppState};

/// Las invitaciones caducan a las 72 horas.
pub const INVITATION_TTL: i64 = 72 * 3600;

#[derive(Deserialize, ToSchema, Default)]
pub struct NewInvitation {
    /// Nota para recordar a quién se envió.
    pub note: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedInvitation {
    #[schema(value_type = String)]
    pub id: Id,
    /// Enlace para enviar a la persona invitada. Solo se muestra una vez.
    pub url: String,
    pub expires_at: i64,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvitationStatus {
    Pending,
    Used,
    Expired,
}

#[derive(Serialize, ToSchema)]
pub struct InvitationOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub note: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub status: InvitationStatus,
}

impl From<Invitation> for InvitationOut {
    fn from(i: Invitation) -> Self {
        let status = match (i.used_at, i.expires_at > now()) {
            (Some(_), _) => InvitationStatus::Used,
            (None, true) => InvitationStatus::Pending,
            (None, false) => InvitationStatus::Expired,
        };
        InvitationOut {
            id: i.id,
            note: i.note,
            created_at: i.created_at,
            expires_at: i.expires_at,
            status,
        }
    }
}

#[utoipa::path(post, path = "/api/v1/invitations", tag = "invitations", request_body = NewInvitation,
    responses((status = 200, body = CreatedInvitation), (status = 403, body = ErrorBody)))]
pub async fn create(
    State(st): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Json(req): Json<NewInvitation>,
) -> ApiResult<Json<CreatedInvitation>> {
    let token = crypto::random_token();
    let note = req.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let inv = st
        .db
        .create_invitation(admin.id, note, &crypto::token_hash(&token), INVITATION_TTL)
        .await?;
    st.db.audit(Some(admin.id), "invitation.created", note).await?;
    let base = st.public_url().await?;
    Ok(Json(CreatedInvitation {
        id: inv.id,
        url: format!("{base}/invite/{token}"),
        expires_at: inv.expires_at,
    }))
}

#[utoipa::path(get, path = "/api/v1/invitations", tag = "invitations",
    responses((status = 200, body = [InvitationOut]), (status = 403, body = ErrorBody)))]
pub async fn list(State(st): State<Arc<AppState>>, _admin: AdminUser) -> ApiResult<Json<Vec<InvitationOut>>> {
    Ok(Json(
        st.db.list_invitations().await?.into_iter().map(Into::into).collect(),
    ))
}

#[utoipa::path(delete, path = "/api/v1/invitations/{id}", tag = "invitations",
    params(("id" = String, Path, description = "Id en Base58")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn revoke(
    State(st): State<Arc<AppState>>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id: Id = id.parse()?;
    if st.db.delete_invitation(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("La invitación no existe o ya se usó."))
    }
}

#[derive(Serialize, ToSchema)]
pub struct InvitationCheck {
    pub valid: bool,
    pub expires_at: Option<i64>,
}

/// Comprueba un enlace de invitación sin usarlo. Público.
#[utoipa::path(get, path = "/api/v1/invite/{token}", tag = "invitations",
    params(("token" = String, Path)), responses((status = 200, body = InvitationCheck)))]
pub async fn check(State(st): State<Arc<AppState>>, Path(token): Path<String>) -> ApiResult<Json<InvitationCheck>> {
    let inv = st.db.valid_invitation(&crypto::token_hash(&token)).await?;
    Ok(Json(InvitationCheck {
        valid: inv.is_some(),
        expires_at: inv.map(|i| i.expires_at),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct AcceptInvitation {
    /// Usuario de Buzón. No tiene relación con las cuentas de correo.
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub language: Option<String>,
}

/// Usa la invitación: crea el usuario de Buzón y abre su sesión. Público.
#[utoipa::path(post, path = "/api/v1/invite/{token}", tag = "invitations",
    params(("token" = String, Path)), request_body = AcceptInvitation,
    responses((status = 200, body = Me), (status = 410, body = ErrorBody, description = "Invitación caducada o ya usada")))]
pub async fn accept(
    State(st): State<Arc<AppState>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Json(req): Json<AcceptInvitation>,
) -> ApiResult<(HeaderMap, Json<Me>)> {
    validate::username(&req.username)?;
    validate::password(&req.password)?;
    let language = match &req.language {
        Some(l) => {
            validate::language(l)?;
            l.clone()
        }
        None => st.db.setting("default_language").await?.unwrap_or_else(|| "es".into()),
    };
    let hash = crypto::hash_password(&req.password)?;
    let user = st
        .db
        .accept_invitation(
            &crypto::token_hash(&token),
            NewUser {
                username: req.username.trim(),
                email: req.email.as_deref().filter(|e| !e.is_empty()),
                password_hash: &hash,
                role: Role::User,
                language: &language,
            },
        )
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::GONE,
                "invitation_invalid",
                "La invitación ha caducado o ya se usó.",
            )
        })?;
    let udb = st.user_db(user.id).await?;
    st.db.audit(Some(user.id), "invitation.accepted", None).await?;
    let cookie = session::start(&st, &user, &headers).await?;
    Ok((cookie, Json(Me::new(&user, prefs::load(&udb).await?))))
}
