#!/usr/bin/python3

import sys, os, socket, selectors

RECV_MAX=4000

def read(socket):
    """Read as much data as possible, then return it."""
    has_eof = False
    db = b''

    try:
        data = b''
        while True:
            data = socket.recv(RECV_MAX)
            db += data
            if len(data) == 0:
                print("EOF condition reached")
                has_eof = True
                break
            if len(data) < RECV_MAX:
                # There wasn't more data than we can get in one call, so there won't *be* more.  (we hope)
                break
    except BlockingIOError: #, ssl.SSLWantReadError, ssl.SSLWantWriteError
        print("Got a BlockingIOError in read() call.")
        pass
    except OSError:
        print("Got an OSError in read() call.")
        has_eof = True
    except ConnectionResetError:
        print("Got a ConnectionResetError in read() call.")
        has_eof = True

    if has_eof:
        socket.close()

    # We could do line buffering or whatever here but....who cares
    return (has_eof, db)

def main():
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.setblocking(0)

    host = 'localhost'
    port = '7072'

    server.bind((host, int(port)))
    server.listen(100)

    sel = selectors.DefaultSelector()
    sel.register(server, selectors.EVENT_READ)

    clients = []

    print("Listening on %s:%s." % (host,port))
    while True:
        events = sel.select(timeout=1)
        for key, mask in events:
            s = key.fileobj

            if s == server:
                (connection, address) = s.accept()
                connection.setblocking(0)
                sel.register(connection, selectors.EVENT_READ)
                clients += [connection]
                print("Got socket " + repr(connection) + "with address" + repr(address))
            else:
                link = s
                (eof, data) = read(s)
                if not eof:
                    s.write(data)
                    print("Got", repr(data))
                if eof:
                    sel.unregister(s)
                    while s in clients:
                        clients.remove(s)
        # Send everyone on the server a message every second or so to simulate real traffic.
        for client in clients:
            client.send(b'Lorem ipsum quod ecit sit dolor remen.  Amputate mixed bias is confusing and inadequate, but spindrift accumulation has been allowing the issue to go unnoticed for the past several thousand head of sheep in a herd where x is not negative.  If x is negative, go to page 99999999999.\nThis is line two of a two-line message.  Welcome to the future!\nSorry, we were just kidding.  It\'s actually three lines...\n')

main()
