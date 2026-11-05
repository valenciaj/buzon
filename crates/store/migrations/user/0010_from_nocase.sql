-- Los remitentes se buscan sin distinguir mayúsculas (senders.email es NOCASE y from_addr
-- conserva las de cada mensaje); con este índice, «Por aprobar» no recorre todo el correo.
CREATE INDEX messages_by_from_nocase ON messages (from_addr COLLATE NOCASE, date);
