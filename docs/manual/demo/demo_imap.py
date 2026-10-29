"""Servidor IMAP de demostración para las capturas del manual (solo lo que usa Buzón).

Uso: python3 demo_imap.py PUERTO es|en CARPETA
Cuando aparece el fichero CARPETA/deliver, entrega los correos de remitentes nuevos
(para que se vean en «Por aprobar»).
"""
import asyncio, base64, os, re, sys, time
from email.utils import formatdate

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 3143
LANG = sys.argv[2] if len(sys.argv) > 2 else "es"
DIR = sys.argv[3] if len(sys.argv) > 3 else "."

def d(days_ago, hour=9):
    return formatdate(time.time() - days_ago * 86400 + (hour - 9) * 3600, localtime=True)

class Folder:
    def __init__(self, name, attrs, validity):
        self.name, self.attrs, self.validity, self.next, self.msgs = name, attrs, validity, 0, []
    def add(self, raw, flags=()):
        self.next += 1
        self.msgs.append({"uid": self.next, "flags": list(flags), "raw": raw.replace("\r\n", "\n").replace("\n", "\r\n").encode()})

F = {f.name: f for f in [Folder("INBOX", r"\HasNoChildren", 11), Folder("Sent", r"\HasNoChildren \Sent", 12),
                          Folder("Drafts", r"\HasNoChildren \Drafts", 13), Folder("Trash", r"\HasNoChildren \Trash", 15),
                          Folder("Archive", r"\HasNoChildren \Archive", 16), Folder("Junk", r"\HasNoChildren \Junk", 17)]}
inbox, sent = F["INBOX"], F["Sent"]
ME = "Javier <yo@ejemplo.es>"

def msg(frm, subject, body, days, hour=9, to=ME, mid=None, extra=""):
    mid = mid or f"{abs(hash((frm, subject, days, hour)))}@demo.example"
    return f"From: {frm}\nTo: {to}\nSubject: {subject}\nMessage-ID: <{mid}>\nDate: {d(days, hour)}\n{extra}\n{body}\n"

PDF = "JVBERi0xLjQKJcOkw7zDtsOfCjEgMCBvYmoKPDwvVHlwZS9DYXRhbG9nPj4KZW5kb2JqCnRyYWlsZXIKPDwvUm9vdCAxIDAgUj4+CiUlRU9GCg=="

def with_pdf(frm, subject, body, days, hour, name):
    return msg(frm, subject, "", days, hour, extra=f'''MIME-Version: 1.0
Content-Type: multipart/mixed; boundary="MIX"

--MIX
Content-Type: text/plain; charset=utf-8

{body}
--MIX
Content-Type: application/pdf; name="{name}"
Content-Disposition: attachment; filename="{name}"
Content-Transfer-Encoding: base64

{PDF}
--MIX--''')

def html(frm, subject, inner, days, hour, extra=""):
    return msg(frm, subject, f"<html><body style=\"font-family:Arial;max-width:560px\">{inner}"
               f"<img src=\"https://tracker.example/open.gif?u=1\" width=\"1\" height=\"1\"></body></html>", days, hour,
               extra=f"MIME-Version: 1.0\nContent-Type: text/html; charset=utf-8\n{extra}")

NEWS = "List-Unsubscribe: <mailto:baja@lista.example>\nList-Id: <lista.example>\n"
T = {
    "es": dict(
        ana="Ana García <ana@ejemplo.es>", luis="Luis Pérez <luis@estudio.example>", marta="Marta Ruiz <marta@trabajo.example>",
        dinner="Cena del viernes", dinner1="Hola Javier,\n\n¿Te viene bien el viernes a las nueve? He visto un sitio nuevo en el centro.\n\nUn abrazo,\nAna",
        dinner2="¡Perfecto! Allí estaré.", dinner3="Genial, reservo mesa para cuatro. Te paso la carta mañana.",
        plans="Planos de la reforma", plans1="Te dejo los planos revisados con la cocina abierta. Dime qué te parecen.", plansf="planos-v2.pdf",
        meet="Reunión del lunes", meet1="¿Podemos mover la reunión del lunes a las 11? Tengo médico a primera hora.",
        news=[("Boletín de Fotografía <hola@foto.example>", "Diez trucos para fotos nocturnas", "Esta semana: trípodes ligeros, ISO alto sin ruido y los mejores sitios para ver estrellas."),
              ("Librería Rayuela <libros@rayuela.example>", "Novedades de noviembre", "Llegan las novelas más esperadas del otoño y un 10 % de descuento para socios."),
              ("Club de Corredores <club@corre.example>", "Calendario de carreras", "Apúntate a la carrera popular del día 15. Quedan 40 dorsales.")],
        shop="Tienda Verde <pedidos@tienda.example>", order="Tu pedido 4815 está en camino",
        order1="<h1 style=\"color:#1d7a4c\">¡Tu pedido va de camino!</h1><p>Llegará el <b>jueves</b>. Puedes seguir el envío desde tu cuenta.</p>",
        power="Luz Clara <facturas@luz.example>", bill="Tu factura de octubre", bill1="Ya tienes disponible tu factura de octubre: 48,20 €.", billf="factura-octubre.pdf",
        late=[("Asociación de Vecinos <junta@vecinos.example>", "Fiesta del barrio", "Hola, te escribimos para invitarte a la fiesta del barrio del sábado. ¿Te apuntas?"),
              ("Paula Martín <paula@agencia.example>", "Propuesta de colaboración", "Hola Javier, me gustaría proponerte un proyecto. ¿Tienes un rato esta semana?")],
    ),
    "en": dict(
        ana="Anna Green <anna@example.com>", luis="Lewis Park <lewis@studio.example>", marta="Martha Reed <martha@work.example>",
        dinner="Dinner on Friday", dinner1="Hi Javier,\n\nDoes Friday at nine work for you? I found a new place downtown.\n\nCheers,\nAnna",
        dinner2="Perfect! I\'ll be there.", dinner3="Great, I\'ll book a table for four. I\'ll send you the menu tomorrow.",
        plans="Renovation plans", plans1="Here are the revised plans with the open kitchen. Let me know what you think.", plansf="plans-v2.pdf",
        meet="Monday\'s meeting", meet1="Can we move Monday\'s meeting to 11? I have a doctor\'s appointment first thing.",
        news=[("Photo Weekly <hello@photo.example>", "Ten tips for night photography", "This week: light tripods, high ISO without noise and the best places to see the stars."),
              ("Hopscotch Books <books@hopscotch.example>", "New in November", "The most awaited novels of the autumn are here, plus 10% off for members."),
              ("Runners Club <club@run.example>", "Race calendar", "Sign up for the fun run on the 15th. 40 bibs left.")],
        shop="Green Store <orders@store.example>", order="Your order 4815 is on its way",
        order1="<h1 style=\"color:#1d7a4c\">Your order is on its way!</h1><p>It arrives on <b>Thursday</b>. You can track it from your account.</p>",
        power="Bright Power <bills@power.example>", bill="Your October bill", bill1="Your October bill is ready: $52.10.", billf="bill-october.pdf",
        late=[("Neighbours Association <board@neighbours.example>", "Street party", "Hi, we\'d like to invite you to the street party on Saturday. Are you in?"),
              ("Paula Martin <paula@agency.example>", "Collaboration proposal", "Hi Javier, I\'d like to propose a project. Do you have some time this week?")],
    ),
}[LANG]

for i, (frm, subj, body) in enumerate(T["news"]):
    inbox.add(msg(frm, subj, body, 1 + i, 7, extra=NEWS))
inbox.add(with_pdf(T["power"], T["bill"], T["bill1"], 2, 10, T["billf"]), ["\\Seen"])
inbox.add(msg(T["luis"], T["plans"], T["plans1"], 3, 16), ["\\Seen"])
inbox.add(msg(T["ana"], T["dinner"], T["dinner1"], 2, 19, mid="a1@demo.example"), ["\\Seen"])
sent.add(msg(ME, "Re: " + T["dinner"], T["dinner2"], 1, 10, to=T["ana"], mid="a2@demo.example",
             extra="In-Reply-To: <a1@demo.example>\nReferences: <a1@demo.example>\n"), ["\\Seen"])
inbox.add(msg(T["ana"], "Re: " + T["dinner"], T["dinner3"], 0, 8, mid="a3@demo.example",
              extra="In-Reply-To: <a2@demo.example>\nReferences: <a1@demo.example> <a2@demo.example>\n"))
inbox.add(msg(T["marta"], T["meet"], T["meet1"], 0, 9))
inbox.add(html(T["shop"], T["order"], T["order1"], 0, 7))
LATE = [msg(frm, subj, body, 0, 10) for frm, subj, body in T["late"]]

waiters = set()

def parse_set(s, mx):
    out = []
    for p in s.split(","):
        a, _, b = p.partition(":")
        n = lambda x: mx if x == "*" else int(x)
        lo, hi = (n(a), n(b)) if b else (n(a), n(a))
        out.append((min(lo, hi), max(lo, hi)))
    return out

def inset(u, r):
    return any(a <= u <= b for a, b in r)

def words(line):
    return [w.strip('"') for w in re.findall(r'"[^"]*"|\([^)]*\)|\S+', line)]

async def client(reader, writer):
    sel = None
    w = lambda s: writer.write(s if isinstance(s, bytes) else s.encode())
    w("* OK fake\r\n")
    while True:
        line = await reader.readline()
        if not line:
            break
        p = words(line.decode().rstrip("\r\n"))
        tag, cmd = p[0], p[1].upper() if len(p) > 1 else ""
        ok = f"{tag} OK hecho\r\n"
        if cmd == "CAPABILITY":
            w("* CAPABILITY IMAP4rev1 IDLE MOVE UIDPLUS\r\n" + ok)
        elif cmd == "LOGIN":
            w(ok if p[3] == "clave" else f"{tag} NO credenciales no válidas\r\n")
        elif cmd == "LOGOUT":
            w("* BYE\r\n" + ok); await writer.drain(); break
        elif cmd == "NOOP":
            w(ok)
        elif cmd == "LIST":
            w("".join(f'* LIST ({f.attrs}) "/" "{f.name}"\r\n' for f in F.values()) + ok)
        elif cmd in ("SELECT", "EXAMINE"):
            sel = F[p[2]]
            w(f"* {len(sel.msgs)} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {sel.validity}] v\r\n* OK [UIDNEXT {sel.next + 1}] n\r\n* FLAGS (\\Seen \\Flagged)\r\n{tag} OK [READ-WRITE] hecho\r\n")
        elif cmd == "UID":
            sub = p[2].upper(); mx = max([m["uid"] for m in sel.msgs] or [0])
            if sub == "SEARCH":
                r = [(0, 1 << 32)] if p[3].upper() == "ALL" else parse_set(p[4], mx)
                w("* SEARCH " + " ".join(str(m["uid"]) for m in sel.msgs if inset(m["uid"], r)) + "\r\n" + ok)
            elif sub == "FETCH":
                r = parse_set(p[3], mx); items = p[4].upper(); out = b""
                for i, m in enumerate(sel.msgs):
                    if not inset(m["uid"], r):
                        continue
                    a = f"UID {m['uid']}"
                    if "FLAGS" in items: a += f" FLAGS ({' '.join(m['flags'])})"
                    if "RFC822.SIZE" in items: a += f" RFC822.SIZE {len(m['raw'])}"
                    out += f"* {i + 1} FETCH ({a}".encode()
                    if "BODY.PEEK[]" in items:
                        out += f" BODY[] {{{len(m['raw'])}}}\r\n".encode() + m["raw"]
                    out += b")\r\n"
                w(out + ok.encode())
            elif sub == "STORE":
                r = parse_set(p[3], mx); add = p[4].startswith("+"); fl = p[5].strip("()").split()
                for m in sel.msgs:
                    if inset(m["uid"], r):
                        m["flags"] = [x for x in m["flags"] if x not in fl] + (fl if add else [])
                print("STORE", sel.name, p[3], p[4], fl, flush=True)
                w(ok)
            elif sub in ("MOVE", "COPY"):
                r = parse_set(p[3], mx); dest = F[p[4]]
                picked = [m for m in sel.msgs if inset(m["uid"], r)]
                for m in picked:
                    dest.add(m["raw"].decode("utf-8", "replace"), m["flags"])
                if sub == "MOVE":
                    for m in picked:
                        i = sel.msgs.index(m); del sel.msgs[i]; w(f"* {i + 1} EXPUNGE\r\n")
                print(sub, sel.name, "->", dest.name, len(picked), flush=True)
                for ev, wr, f in list(waiters):
                    if f is dest:
                        wr.write(f"* {len(dest.msgs)} EXISTS\r\n".encode())
                w(ok)
            elif sub == "EXPUNGE":
                r = parse_set(p[3], mx)
                gone = [i for i, m in enumerate(sel.msgs) if inset(m["uid"], r) and "\\Deleted" in m["flags"]]
                for i in reversed(gone):
                    del sel.msgs[i]; w(f"* {i + 1} EXPUNGE\r\n")
                print("UID EXPUNGE", sel.name, len(gone), flush=True)
                w(ok)
        elif cmd == "EXPUNGE":
            gone = [i for i, m in enumerate(sel.msgs) if "\\Deleted" in m["flags"]]
            for i in reversed(gone):
                del sel.msgs[i]; w(f"* {i + 1} EXPUNGE\r\n")
            print("EXPUNGE", sel.name, len(gone), flush=True)
            w(ok)
        elif cmd == "APPEND":
            n = int(re.search(r"\{(\d+)\+?\}", line.decode()).group(1))
            if "+}" not in line.decode():
                w("+ adelante\r\n"); await writer.drain()
            data = await reader.readexactly(n); await reader.readline()
            folder = F[p[2]]; folder.add(data.decode("utf-8", "replace"), ["\\Seen"])
            print("APPEND", folder.name, n, flush=True)
            for ev, wr, f in list(waiters):
                if f is folder:
                    wr.write(f"* {len(folder.msgs)} EXISTS\r\n".encode())
            w(ok)
        elif cmd == "IDLE":
            w("+ idling\r\n"); await writer.drain()
            ev = asyncio.Event(); waiters.add((ev, writer, sel))
            await reader.readline(); waiters.discard((ev, writer, sel))
            w(ok)
        else:
            w(f"{tag} BAD no\r\n")
        await writer.drain()
    writer.close()

async def deliver():
    """Cuando aparece CARPETA/deliver, entrega los correos de remitentes nuevos."""
    path = os.path.join(DIR, "deliver")
    while True:
        await asyncio.sleep(0.5)
        if os.path.exists(path):
            os.remove(path)
            for raw in LATE:
                inbox.add(raw)
            for ev, writer, folder in list(waiters):
                if folder is inbox:
                    writer.write(f"* {len(inbox.msgs)} EXISTS\r\n".encode())
            print("entregados", len(LATE), flush=True)

async def main():
    srv = await asyncio.start_server(client, "127.0.0.1", PORT)
    print("IMAP de demostración en", PORT, flush=True)
    asyncio.create_task(deliver())
    async with srv:
        await srv.serve_forever()

asyncio.run(main())
