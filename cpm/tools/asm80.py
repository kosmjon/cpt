#!/usr/bin/env python3
"""Minimal Intel-8080 cross assembler, just enough to build the CP/M 2.2 CCP and BDOS
(cpm/vendor/ccp.asm, bdos.asm, in Macro Assembler AS style).

This is a build helper, NOT part of MOL80: the CP/M system itself is stock Digital Research
code, only the BIOS is written in MOL80.

Supported: all 8080 mnemonics; labels (with or without ':'); EQU, SET, ORG, DS, DB, DW, END;
IF / IFDEF / IFNDEF / ELSE / ENDIF; TITLE and .CPU are ignored.  Expressions: numbers
(nnnH, nnnB, decimal, 'c'), $, symbols, + - * / ~ & | ^ ( ), and the words MOD SHR SHL AND OR
XOR NOT LOW HIGH.

usage: asm80.py [-Dname[=value]] -o out.bin [-s out.sym.json] src.asm [src2.asm ...]
Each source is assembled separately, in order; the outputs are concatenated by address into
one image starting at the lowest ORG (gaps zero-filled).
"""
import json
import re
import sys

REG = {"b": 0, "c": 1, "d": 2, "e": 3, "h": 4, "l": 5, "m": 6, "a": 7}
RP = {"b": 0, "d": 1, "h": 2, "sp": 3}
RP_STK = {"b": 0, "d": 1, "h": 2, "psw": 3}

SIMPLE = {
    "nop": 0x00, "rlc": 0x07, "rrc": 0x0F, "ral": 0x17, "rar": 0x1F, "daa": 0x27,
    "cma": 0x2F, "stc": 0x37, "cmc": 0x3F, "hlt": 0x76, "xchg": 0xEB, "xthl": 0xE3,
    "sphl": 0xF9, "pchl": 0xE9, "ei": 0xFB, "di": 0xF3, "ret": 0xC9,
    "rnz": 0xC0, "rz": 0xC8, "rnc": 0xD0, "rc": 0xD8, "rpo": 0xE0, "rpe": 0xE8,
    "rp": 0xF0, "rm": 0xF8,
}
ALU_REG = {"add": 0x80, "adc": 0x88, "sub": 0x90, "sbb": 0x98,
           "ana": 0xA0, "xra": 0xA8, "ora": 0xB0, "cmp": 0xB8}
ALU_IMM = {"adi": 0xC6, "aci": 0xCE, "sui": 0xD6, "sbi": 0xDE,
           "ani": 0xE6, "xri": 0xEE, "ori": 0xF6, "cpi": 0xFE}
ADDR3 = {
    "jmp": 0xC3, "jnz": 0xC2, "jz": 0xCA, "jnc": 0xD2, "jc": 0xDA, "jpo": 0xE2,
    "jpe": 0xEA, "jp": 0xF2, "jm": 0xFA,
    "call": 0xCD, "cnz": 0xC4, "cz": 0xCC, "cnc": 0xD4, "cc": 0xDC, "cpo": 0xE4,
    "cpe": 0xEC, "cp": 0xF4, "cm": 0xFC,
    "lda": 0x3A, "sta": 0x32, "lhld": 0x2A, "shld": 0x22,
}


class AsmError(Exception):
    pass


def split_args(text):
    """Split on top-level commas (not inside quotes or parentheses)."""
    out, cur, depth, quote = [], "", 0, None
    for ch in text:
        if quote:
            cur += ch
            if ch == quote:
                quote = None
        elif ch in "'\"":
            quote = ch
            cur += ch
        elif ch == "(":
            depth += 1
            cur += ch
        elif ch == ")":
            depth -= 1
            cur += ch
        elif ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def strip_comment(line):
    quote = None
    for i, ch in enumerate(line):
        if quote:
            if ch == quote:
                quote = None
        elif ch in "'\"":
            quote = ch
        elif ch == ";":
            return line[:i]
    return line


class Assembler:
    def __init__(self, defines):
        self.defines = defines
        self.symbols = {}
        self.final = False  # True during the second pass
        self.pc = 0
        self.mem = {}
        self.org_start = None

    # ---- expressions -------------------------------------------------------------------
    def eval(self, text):
        text = text.strip()
        py, i = [], 0
        while i < len(text):
            ch = text[i]
            if ch.isspace():
                i += 1
            elif ch == "'" or ch == '"':
                j = text.index(ch, i + 1)
                s = text[i + 1:j]
                val = 0
                for c in s:
                    val = (val << 8) | ord(c)
                py.append(str(val))
                i = j + 1
            elif ch == "$":
                py.append(str(self.pc))
                i += 1
            elif ch.isdigit():
                m = re.match(r"[0-9][0-9a-zA-Z]*", text[i:])
                tok = m.group(0).lower()
                if tok.endswith("h"):
                    val = int(tok[:-1], 16)
                elif tok.endswith("b") and set(tok[:-1]) <= set("01"):
                    val = int(tok[:-1], 2)
                else:
                    val = int(tok, 10)
                py.append(str(val))
                i += m.end()
            elif ch.isalpha() or ch == "_" or ch == ".":
                m = re.match(r"[A-Za-z_.][A-Za-z0-9_.]*", text[i:])
                name = m.group(0).lower()
                i += m.end()
                words = {"mod": "%", "shr": ">>", "shl": "<<", "and": "&", "or": "|",
                         "xor": "^", "not": "~"}
                if name in words:
                    py.append(words[name])
                elif name in ("low", "high"):
                    py.append("_" + name)
                else:
                    py.append("_S(%r)" % name)
            elif ch == "/":
                py.append("//")
                i += 1
            elif ch in "+-*()~&|^":
                py.append(ch)
                i += 1
            else:
                raise AsmError("bad character %r in expression %r" % (ch, text))
        env = {
            "_S": self.lookup,
            "_low": lambda x: x & 0xFF,
            "_high": lambda x: (x >> 8) & 0xFF,
        }
        expr = " ".join(py)
        # `low x` / `high x` without parentheses.
        expr = re.sub(r"_(low|high) (?!\()", r"_\1 (", expr) if False else expr
        try:
            return int(eval(expr, {"__builtins__": {}}, env))
        except AsmError:
            raise
        except Exception as e:  # noqa: BLE001
            raise AsmError("cannot evaluate %r (%s)" % (text, e))

    def lookup(self, name):
        if name in self.symbols:
            return self.symbols[name]
        if name in self.defines:
            return self.defines[name]
        if self.final:
            raise AsmError("undefined symbol %r" % name)
        return 0

    def defined(self, name):
        return name in self.symbols or name in self.defines

    # ---- emission ----------------------------------------------------------------------
    def emit(self, *bs):
        for b in bs:
            if self.final:
                self.mem[self.pc] = b & 0xFF
            self.pc += 1

    def word(self, v):
        v &= 0xFFFF
        self.emit(v & 0xFF, v >> 8)

    def reg(self, s):
        r = REG.get(s.strip().lower())
        if r is None:
            raise AsmError("expected register, got %r" % s)
        return r

    def rp(self, s, table=RP):
        r = table.get(s.strip().lower())
        if r is None:
            raise AsmError("expected register pair, got %r" % s)
        return r

    def imm8(self, s):
        v = self.eval(s)
        if not -256 <= v <= 255:
            raise AsmError("value %d does not fit in a byte" % v)
        return v & 0xFF

    # ---- driver ------------------------------------------------------------------------
    def run_pass(self, lines, final):
        self.final = final
        self.pc = 0
        cond = []  # stack of [active_now, taken_any, parent_active]
        active = True
        for lineno, raw in enumerate(lines, 1):
            try:
                line = strip_comment(raw).rstrip()
                if not line.strip():
                    continue
                label, rest = None, line
                m = None
                if not line[0].isspace():
                    m = re.match(r"([A-Za-z_.][A-Za-z0-9_.]*)\s*:?", line)
                    label = m.group(1).lower()
                    rest = line[m.end():]
                rest = rest.strip()
                if rest:
                    parts = rest.split(None, 1)
                    op = parts[0].lower()
                    args = parts[1].strip() if len(parts) > 1 else ""
                else:
                    op, args = "", ""

                # Conditional assembly is processed even in inactive regions.
                if op in ("if", "ifdef", "ifndef"):
                    if active:
                        if op == "if":
                            ok = self.eval(args) != 0
                        elif op == "ifdef":
                            ok = self.defined(args.strip().lower())
                        else:
                            ok = not self.defined(args.strip().lower())
                    else:
                        ok = False
                    cond.append([active, ok])
                    active = active and ok
                    continue
                if op == "else":
                    parent, taken = cond[-1]
                    active = parent and not taken
                    cond[-1][1] = True
                    continue
                if op == "endif":
                    parent, _ = cond.pop()
                    active = parent
                    continue
                if not active:
                    continue

                if op in ("equ", "set"):
                    if label is None:
                        raise AsmError("%s needs a name" % op)
                    self.symbols[label] = self.eval(args)
                    continue
                if label is not None:
                    if final and self.symbols.get(label) not in (None, self.pc):
                        raise AsmError("label %r moved between passes" % label)
                    self.symbols[label] = self.pc
                if not op:
                    continue
                if op in ("title", ".cpu"):
                    continue
                if op == "end":
                    break
                self.statement(op, args)
            except AsmError as e:
                raise AsmError("line %d: %s\n    %s" % (lineno, e, raw.strip()))
        if cond:
            raise AsmError("unterminated IF")

    def statement(self, op, args):
        a = split_args(args)
        if op == "org":
            self.pc = self.eval(args) & 0xFFFF
            if self.org_start is None:
                self.org_start = self.pc
        elif op == "ds":
            self.pc += self.eval(args)
        elif op == "db":
            for item in a:
                if item[:1] in "'\"" and len(item) >= 2 and item[-1] == item[0] and len(item) != 3:
                    self.emit(*[ord(c) for c in item[1:-1]])
                else:
                    self.emit(self.imm8(item))
        elif op == "dw":
            for item in a:
                self.word(self.eval(item))
        elif op in SIMPLE:
            self.emit(SIMPLE[op])
        elif op == "mov":
            self.emit(0x40 | self.reg(a[0]) << 3 | self.reg(a[1]))
        elif op == "mvi":
            self.emit(0x06 | self.reg(a[0]) << 3, self.imm8(a[1]))
        elif op == "lxi":
            self.emit(0x01 | self.rp(a[0]) << 4)
            self.word(self.eval(a[1]))
        elif op in ("ldax", "stax"):
            r = a[0].strip().lower()
            if r not in ("b", "d"):
                raise AsmError("%s takes B or D" % op)
            self.emit((0x0A if op == "ldax" else 0x02) | (0x10 if r == "d" else 0))
        elif op == "push":
            self.emit(0xC5 | self.rp(a[0], RP_STK) << 4)
        elif op == "pop":
            self.emit(0xC1 | self.rp(a[0], RP_STK) << 4)
        elif op == "dad":
            self.emit(0x09 | self.rp(a[0]) << 4)
        elif op == "inx":
            self.emit(0x03 | self.rp(a[0]) << 4)
        elif op == "dcx":
            self.emit(0x0B | self.rp(a[0]) << 4)
        elif op == "inr":
            self.emit(0x04 | self.reg(a[0]) << 3)
        elif op == "dcr":
            self.emit(0x05 | self.reg(a[0]) << 3)
        elif op in ALU_REG:
            self.emit(ALU_REG[op] | self.reg(a[0]))
        elif op in ALU_IMM:
            self.emit(ALU_IMM[op], self.imm8(a[0]))
        elif op in ADDR3:
            self.emit(ADDR3[op])
            self.word(self.eval(a[0]))
        elif op == "rst":
            self.emit(0xC7 | (self.eval(a[0]) & 7) << 3)
        elif op == "in":
            self.emit(0xDB, self.imm8(a[0]))
        elif op == "out":
            self.emit(0xD3, self.imm8(a[0]))
        else:
            raise AsmError("unknown mnemonic %r" % op)

    def assemble(self, text):
        lines = text.splitlines()
        self.symbols = {}
        self.run_pass(lines, final=False)
        self.run_pass(lines, final=True)


def main(argv):
    defines, out, sym_out, srcs = {}, None, None, []
    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg.startswith("-D"):
            name, _, val = arg[2:].partition("=")
            defines[name.lower()] = int(val, 0) if val else 1
        elif arg == "-o":
            i += 1
            out = argv[i]
        elif arg == "-s":
            i += 1
            sym_out = argv[i]
        else:
            srcs.append(arg)
        i += 1
    if not out or not srcs:
        print(__doc__)
        return 2

    mem, symbols = {}, {}
    for path in srcs:
        asm = Assembler(defines)
        with open(path, encoding="latin-1") as f:
            asm.assemble(f.read())
        overlap = set(mem) & set(asm.mem)
        if overlap:
            raise SystemExit("%s overlaps earlier output at %#06x" % (path, min(overlap)))
        mem.update(asm.mem)
        for k, v in asm.symbols.items():
            symbols.setdefault(k, v)
        # Make earlier modules' symbols visible to later ones (e.g. bdos needs nothing, but
        # keeps -D style cross references possible).
    lo, hi = min(mem), max(mem)
    image = bytearray(hi - lo + 1)
    for addr, b in mem.items():
        image[addr - lo] = b
    with open(out, "wb") as f:
        f.write(image)
    if sym_out:
        with open(sym_out, "w") as f:
            json.dump({"base": lo, "length": len(image), "symbols": symbols}, f, indent=1,
                      sort_keys=True)
    print("%s: %#06x-%#06x (%d bytes)" % (out, lo, hi, len(image)))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except AsmError as e:
        sys.exit("error: %s" % e)
