"""Serve the CPT web page (crates/cpt-wasm/web) for run-web.bat.

Like `python -m http.server`, plus `Cache-Control: no-cache` on every file, so a browser checks
with the server on each load (a cheap "304 not modified" when nothing changed) instead of
reusing an old cpt.js or cpt_wasm.wasm after a rebuild.

    python tools/serve_web.py [port] [directory]
"""

import functools
import http.server
import sys


class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()


def main(argv):
    port = int(argv[0]) if argv else 8080
    directory = argv[1] if len(argv) > 1 else "crates/cpt-wasm/web"
    handler = functools.partial(NoCacheHandler, directory=directory)
    with http.server.ThreadingHTTPServer(("", port), handler) as httpd:
        print("Serving %s at http://localhost:%d  (Ctrl+C to stop)" % (directory, port))
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main(sys.argv[1:])
