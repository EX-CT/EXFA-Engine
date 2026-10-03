#!/usr/bin/env python3
"""Remove the given object keys from engine JSON output text, byte-preserving everything else.

Used by run_suites.sh to prove that an output-extending change (new stats-ext fields) left every
pre-existing byte of the round-1 corpus untouched: strip the new keys, compare the sha256 with
ci/round1-base.sha256 (the hash before the fields existed).

Paths are dotted object keys from the top of each line; `[]` steps into every array element:
  mining   outgoing   drones.items[].hp   modules[].heat
Usage: strip_keys.py PATH... < in.jsonl > out.jsonl  (one JSON value per line)
"""
import sys


def emit_value(i, out, paths, p, line):
    """copy one value starting at i (recursing into containers), return index after it"""
    c = line[i]
    if c == '{':
        sub_out = []
        j = obj(i, sub_out, paths, p, line)
        out.append(''.join(sub_out))
        return j
    if c == '[':
        out.append('['); i += 1
        first = True
        while line[i] != ']':
            if line[i] == ',':
                i += 1
            if not first:
                out.append(',')
            first = False
            i = emit_value(i, out, paths, p + "[]", line)
        out.append(']')
        return i + 1
    if c == '"':
        j = i + 1
        while line[j] != '"':
            j += 2 if line[j] == '\\' else 1
        out.append(line[i:j + 1])
        return j + 1
    j = i
    while j < len(line) and line[j] not in ',}]':
        j += 1
    out.append(line[i:j])
    return j


def obj(i, out, paths, path, line):
    assert line[i] == '{'
    out.append('{'); i += 1
    first = True
    while line[i] != '}':
        if line[i] == ',':
            i += 1
        k0 = i
        i += 1
        while line[i] != '"':
            i += 2 if line[i] == '\\' else 1
        i += 1
        key = line[k0 + 1:i - 1]
        assert line[i] == ':', (i, line[i - 20:i + 20])
        p = (path + "." if path else "") + key
        if p in paths:
            sink = []
            i = emit_value(i + 1, sink, paths, p, line)
            continue
        out.append(("" if first else ",") + line[k0:i + 1])
        first = False
        i = emit_value(i + 1, out, paths, p, line)
    out.append('}')
    return i + 1


def main():
    paths = set(sys.argv[1:])
    for line in sys.stdin:
        nl = line.endswith('\n')
        s = line[:-1] if nl else line
        if s.startswith('{'):
            buf = []
            j = obj(0, buf, paths, "", s)
            s = ''.join(buf) + s[j:]
        sys.stdout.write(s + ('\n' if nl else ''))


if __name__ == '__main__':
    main()
