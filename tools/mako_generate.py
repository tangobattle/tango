#!/usr/bin/env python3
import os
import mako.template
import sys

if __name__ == "__main__":
    path = os.path.realpath(sys.argv[1])
    with open(path, "rb") as f:
        raw = f.read()
    tmpl = mako.template.Template(raw)
    # Force UTF-8 on stdout. On Windows the default is cp1252 which
    # can't encode characters that appear in some of our templates
    # (e.g. the localized strings embedded into the NSIS installer).
    sys.stdout.buffer.write(tmpl.render(__file__=path).encode("utf-8"))
