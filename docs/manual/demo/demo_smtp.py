"""Servidor SMTP de demostración para las capturas del manual: acepta todo y no envía nada."""
import asyncio, sys
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 3025

async def client(reader, writer):
    w = lambda s: writer.write(s.encode())
    w("220 falso ESMTP\r\n")
    rcpt, frm = [], ""
    while True:
        line = await reader.readline()
        if not line:
            break
        cmd = line.decode().rstrip("\r\n")
        up = cmd.upper()
        if up.startswith("EHLO") or up.startswith("HELO"):
            w("250-falso\r\n250-8BITMIME\r\n250 AUTH PLAIN LOGIN\r\n")
        elif up.startswith("AUTH"):
            w("235 vale\r\n")
        elif up.startswith("MAIL FROM"):
            frm, rcpt = cmd[10:], []; w("250 vale\r\n")
        elif up.startswith("RCPT TO"):
            rcpt.append(cmd[8:]); w("250 vale\r\n")
        elif up == "DATA":
            w("354 adelante\r\n"); await writer.drain()
            data = b""
            while True:
                l = await reader.readline()
                if l == b".\r\n":
                    break
                data += l
            print("recibido", len(data), "bytes para", rcpt, flush=True)
            w("250 encolado\r\n")
        elif up == "QUIT":
            w("221 adios\r\n"); await writer.drain(); break
        else:
            w("250 vale\r\n")
        await writer.drain()
    writer.close()

async def main():
    srv = await asyncio.start_server(client, "127.0.0.1", PORT)
    print("fake SMTP en", PORT, flush=True)
    async with srv:
        await srv.serve_forever()

asyncio.run(main())
