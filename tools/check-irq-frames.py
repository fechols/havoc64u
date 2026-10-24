#!/usr/bin/env python3
"""Fail the build when interrupt code could touch a compiler static frame.

LLVM-MOS gives non-reentrant functions static local frames in `.noinit`
(the anonymous `.Lstatic_stack` object). The interrupt wrapper
(target-c64/asm/platform.S) no longer backs those frames up around
`irq_body`, so nothing reachable from the interrupt may use one: the game
would otherwise corrupt a foreground frame whenever the 50 Hz tick lands
inside the owning function. This audit follows every `jsr`/`jmp` from the
interrupt roots through the linked ELF and rejects a compiled function in
that closure whose operands address the static-frame bytes. Hand-written
kernels (plain C symbol names) cannot reference the compiler's local label,
so only their call targets are followed; data tables inside them would
otherwise disassemble into spurious operands. Usage:
check-irq-frames.py [havoc.elf] [--root irq_body ...]
Exit status 1 when any interrupt-reachable compiled function uses a frame.
"""
import argparse
import os
import pathlib
import re
import runpy
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[1]
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home() / ".local/llvm-mos"))) / "bin"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("elf", nargs="?", type=pathlib.Path, default=root / "dist/havoc.elf")
parser.add_argument("--root", action="append", default=None,
                    help="interrupt entry symbol (default irq_body and nmi_entry)")
parser.add_argument("--objdump", default=os.environ.get("MOS_OBJDUMP", str(sdk / "llvm-objdump")))
args = parser.parse_args()
roots = args.root or ["irq_body", "nmi_entry"]

elf = runpy.run_path(str(pathlib.Path(__file__).with_name("check-overlays.py")))["Elf"](args.elf)
noinit = next((s for s in elf.sections if s.name == ".noinit"), None)
if noinit is None or noinit.size == 0:
    print("check-irq-frames: no .noinit section; nothing to audit")
    sys.exit(0)
# The static-frame bytes are .noinit minus the named statics placed there.
frames = [(noinit.address, noinit.address + noinit.size)]
for symbol in elf.symbols:
    if symbol.section == noinit.index and symbol.size:
        begin, end = symbol.address, symbol.address + symbol.size
        frames = [piece for lo, hi in frames for piece in ((lo, min(hi, begin)), (max(lo, end), hi)) if piece[0] < piece[1]]
frame_bytes = sum(hi - lo for lo, hi in frames)
if not frame_bytes:
    print("check-irq-frames: .noinit holds only named statics; nothing to audit")
    sys.exit(0)

listing = subprocess.check_output([args.objdump, "-d", "--no-show-raw-insn", str(args.elf)], text=True)
functions, name = {}, None
for line in listing.splitlines():
    header = re.match(r"^([0-9a-f]+) <(.+)>:$", line)
    if header:
        name = header.group(2)
        functions[name] = []
    elif name and line.strip():
        functions[name].append(line)
compiled = lambda symbol: symbol.startswith("_ZN") or symbol.startswith("__") or symbol in ("main", "irq_body")
target_pattern = re.compile(r"\b(?:jsr|jmp)\s+\$[0-9a-f]+\s+<([^>+]+)(\+0x[0-9a-f]+)?>")
operand_pattern = re.compile(r"\$([0-9a-f]{1,4})\b")

seen, order, queue = set(), [], list(roots)
for symbol in roots:
    if symbol not in functions:
        print(f"check-irq-frames: interrupt root {symbol} is not in the ELF", file=sys.stderr)
        sys.exit(1)
while queue:
    symbol = queue.pop(0)
    if symbol in seen or symbol not in functions:
        continue
    seen.add(symbol)
    order.append(symbol)
    for line in functions[symbol]:
        target = target_pattern.search(line)
        if target and (compiled(symbol) or not target.group(2)):
            queue.append(target.group(1))
problems = 0
for symbol in order:
    if not compiled(symbol):
        continue
    hits = []
    for line in functions[symbol]:
        for value in operand_pattern.findall(line.split(";")[0]):
            address = int(value, 16)
            if any(lo <= address < hi for lo, hi in frames):
                hits.append(line.strip())
    if hits:
        problems += 1
        print(f"STATIC FRAME IN INTERRUPT CLOSURE: {symbol}", file=sys.stderr)
        for hit in hits[:8]:
            print(f"    {hit}", file=sys.stderr)
print(f"check-irq-frames: {len(order)} functions reachable from {', '.join(roots)}; "
      f"{frame_bytes} static-frame bytes at ${frames[0][0]:04x}; "
      f"{sum(compiled(s) for s in order)} compiled functions scanned; problems: {problems}")
sys.exit(1 if problems else 0)
