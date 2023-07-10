#!/usr/bin/env python3
import os
import mako.template
import sys

if __name__ == "__main__":
    path = os.path.realpath(sys.argv[1])
    with open(path, "rb") as f:
        raw = f.read()
    tmpl = mako.template.Template(raw)
    sys.stdout.write(tmpl.render(__file__=path))
