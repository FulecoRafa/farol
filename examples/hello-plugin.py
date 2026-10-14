#!/usr/bin/env python3
"""A farol plugin in plain Python: a tessela service with `query` and `activate`.

Run it beside farol (or as a `[plugins.hello]` in tessela.toml), then tell
farol about it:

    farol set plugins '[{"name": "hello", "title": "olá", "prefix": "oi ", "mixed": 2}]'

and restart farol. `oi mundo` lists its rows; with "mixed" they also show
in the normal search.
"""
import json
import os
import socket
import threading

NAME = "hello"


def query(args):
    text = args.get("text", "")
    if not text:
        return []
    return [
        {"id": "greet:" + text, "title": f"olá, {text}!", "subtitle": "um plugin em Python",
         "icon": "ph:hand-waving", "actions": [{"id": "copy", "title": "copiar saudação", "key": "enter"}]},
    ]


def activate(args):
    greeting = "olá, " + args["id"].split(":", 1)[1] + "!"
    return {"close": True, "toast": greeting}


def serve(conn):
    with conn, conn.makefile("rw") as f:
        for line in f:
            req = json.loads(line)
            method = {"query": query, "activate": activate}.get(req.get("method"))
            reply = {"id": req.get("id")}
            if req.get("op") == "call" and method:
                reply["result"] = method(req.get("args") or {})
            else:
                reply["error"] = "unknown"
            f.write(json.dumps(reply) + "\n")
            f.flush()


def main():
    base = os.environ.get("TESSELA_SERVICES_DIR") or os.path.join(
        os.environ["XDG_RUNTIME_DIR"], "tessela", os.environ.get("FAROL_PROJECT", "farol"), "services")
    path = os.path.join(base, NAME + ".sock")
    if os.path.exists(path):
        os.unlink(path)
    srv = socket.socket(socket.AF_UNIX)
    srv.bind(path)
    srv.listen()
    while True:
        conn, _ = srv.accept()
        threading.Thread(target=serve, args=(conn,), daemon=True).start()


if __name__ == "__main__":
    main()
