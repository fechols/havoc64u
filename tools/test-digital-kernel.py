#!/usr/bin/env python3
"""Execute the production AUDIO overlay against bounded REU/sampler models.

This is a native-code integration test, not a hardware timing measurement.
CPU instructions run at 64 times the PAL/NTSC bus clock; DMA costs one bus
cycle per byte. The sampler model adds two divider ticks of synthetic backend
overhead to the source's 6.25 MHz clock, exercising the calibration correction;
that overhead is not a claim about physical sampler hardware. Muted
one-shot calibration advances time directly to completion on its status poll.
No production decoder, state offsets, or Rust host fixtures are used as the
PCM oracle. Run after scripts/build-target.sh and packing dist/havoc.reu.
The absent-sampler cases separately verify the explicit emulator SID-only mode
and strict production mapping failure; --emulator-only runs just those cases.
"""
import argparse
import hashlib
import json
from pathlib import Path
import runpy
import struct

ROOT = Path(__file__).resolve().parents[1]
KERNEL = runpy.run_path(str(ROOT / "tools/test-music-kernel.py"))
Cpu, KernelError, load_elf = (KERNEL[name] for name in ("Cpu", "KernelError", "load_elf"))
SENTINEL = 0xffff


def require(condition, message):
    if not condition:
        raise KernelError(message)


def pcm_reference(blob):
    """Independent signed-magnitude DLT3 stereo decoder, looping at exact length."""
    samples = int.from_bytes(blob[8:12], "little")
    books = int.from_bytes(blob[18:20], "little")
    segment = (samples // books // 2) * 2
    require(blob[:4] == b"DLT3" and blob[21] == 2, "stereo source profile")
    stream = 32 + books * 96
    while True:
        accumulator = [0, 0]
        for index in range(samples):
            book = min(index // segment, books - 1)
            packed = blob[stream + index]
            for channel in range(2):
                code = (packed >> (4 * channel)) & 15
                entry = 32 + book * 96 + channel * 48 + code * 3
                magnitude = int.from_bytes(blob[entry:entry + 2], "little")
                delta = -min(magnitude, 32768) if blob[entry + 2] else min(magnitude, 32767)
                accumulator[channel] = min(32767, max(-32768, accumulator[channel] + delta))
            yield tuple(accumulator)


class Machine(Cpu):
    def __init__(self, elf, symbols, image, standard, *, sampler_present=True):
        self.symbols = symbols
        self.sections = {s.name: s for s in elf.sections}
        memory = bytearray(65536)
        for name in (*KERNEL["RESIDENT_SECTIONS"], ".phase.audio"):
            section = self.sections[name]
            memory[section.address:section.address + section.size] = elf.contents(section)
        writable = [(0, 0x200), (0x8e00, 0x9300), (0xd400, 0xd480),
                    (0xdd00, 0xdd10), (0xdf00, 0xdf60)]
        for name in (".data", ".bss", ".noinit", ".audio_state", ".game_state", ".music_page"):
            section = self.sections.get(name)
            if section:
                writable.append((section.address, section.address + section.size))
        for name in ("havoc_c64::AUDIO", "havoc_c64::GAME", "havoc_c64::ERROR_DIGITAL"):
            symbol = symbols[name]
            writable.append((symbol.address, symbol.address + symbol.size))
        writable += [(symbols["__stack_bottom"].address, symbols["__stack"].address),
                     (symbols["__irq_stack_bottom"].address, symbols["__irq_stack"].address),
                     (symbols["__music_page_start"].address, symbols["__music_page_end"].address),
                     (symbols["music_kernel_tick"].address,
                      symbols["music_kernel_tick"].address + symbols["__music_kernel_size"].address)]
        super().__init__(memory, writable)
        self.allowed = bytearray(65536)
        for start, end in writable:
            self.allowed[start:end] = b"\1" * (end - start)
        self.reu = bytearray(image)
        self.ring = symbols["__reu_digital_ring"].address
        self.ring_size = symbols["__reu_digital_ring_size"].address
        self.bus_hz = 985248 if standard == "pal" else 1022727
        self.extra_ticks = 0
        self.timer_origin = 0
        self.deadline = None
        self.sampler_present = sampler_present
        self.sampler_reads = self.sampler_writes = 0
        self.control = [0, 0]
        self.volume = [0, 0]
        self.starts = [0, 0]
        self.calibrations = 0
        self.playback = [None, None]
        self.playback_checked = [0, 0]
        self.fetches = self.stashes = self.max_dma = self.ring_wraps = 0
        self.ring_banks = set()
        self.selected = None
        self.references = {}
        for name, offset in (("galactic-dash", symbols["__reu_digital_music"].address),
                             ("hammond-rocker", symbols["__reu_digital_victory"].address)):
            self.references[offset] = (name, (ROOT / f"assets/music/{name}.dlt").read_bytes())
        self.reference = None
        self.produced = 0
        self.pcm_samples = {name: 0 for name, _ in self.references.values()}
        self.calls = []
        self.put("MUSIC_BACKEND", 2)
        self.put("havoc_c64::REQUESTED_CUE", 0)
        self.put("havoc_c64::MUSIC_VALIDATED", 0)
        self.put("havoc_c64::CLOCK_KHZ", 985 if standard == "pal" else 1023, 2)
        a = b = 0
        for value in image[0xc0000:0xc0100]:
            a = (a + value) % 255
            b = (b + a) % 255
        self.put("havoc_c64::MUSIC_DIRECTORY_HASH", a | b << 8, 2)
        stack = symbols["__stack"].address
        self.stack_bottom = symbols["__stack_bottom"].address
        self.stack_top = self.stack_min = stack
        self.mem[2:4] = stack.to_bytes(2, "little")

    @property
    def ticks(self):
        return self.cycles // 64 + self.extra_ticks

    def put(self, name, value, length=1):
        symbol = self.symbols.get(name) or self.symbols[name.rsplit("::", 1)[-1]]
        start = symbol.address
        self.mem[start:start + length] = value.to_bytes(length, "little")

    def get(self, name, length=1):
        symbol = self.symbols.get(name) or self.symbols[name.rsplit("::", 1)[-1]]
        start = symbol.address
        return int.from_bytes(self.mem[start:start + length], "little")

    def read(self, address):
        address &= 0xffff
        if 0xdd04 <= address <= 0xdd07:
            timer = (0xffffffff - (self.ticks - self.timer_origin)) & 0xffffffff
            return (timer >> (8 * (address - 0xdd04))) & 255
        if address == 0xdf21:
            self.sampler_reads += 1
            # Read-only sampler version shares its volume address.
            return 0x10 if self.sampler_present else 0xff
        if address == 0xdf20:
            self.sampler_reads += 1
            if not self.sampler_present:
                return 0xff
            if self.deadline is not None:
                require(self.volume == [0, 0], "calibration must be muted")
                self.extra_ticks += max(0, self.deadline - self.ticks)
                return 1
            return 0
        return self.mem[address]

    def write(self, address, value):
        address &= 0xffff
        require(self.allowed[address], f"store outside owned memory ${address:04x}, PC ${self.pc:04x}")
        if address in (0xdf20, 0xdf40):
            self.flush_playback()
        self.mem[address] = value & 255
        if 0xdf20 <= address <= 0xdfff:
            self.sampler_writes += 1
            if not self.sampler_present:
                return
        if self.stack_bottom <= address < self.stack_min:
            self.stack_min = address
        if address == 0xdd0f and value == 0x51:
            self.timer_origin = self.ticks
        elif address in (0xdf21, 0xdf41):
            self.volume[(address - 0xdf21) // 0x20] = value
        elif address in (0xdf20, 0xdf40):
            channel = (address - 0xdf20) // 0x20
            self.control[channel] = value
            self.playback[channel] = None
            if channel == 0:
                self.deadline = None
            if value == 0x55:
                require(channel == 0, "only left calibration requests completion IRQ")
                frames = self.be24(address + 9) // 4
                divider = int.from_bytes(self.mem[address + 14:address + 16], "big")
                self.deadline = self.ticks + round(frames * self.bus_hz * (divider + 2) / 6250000)
                self.calibrations += 1
            elif value == 0x51:
                require(channel == 1 and self.control[0] == 0x55, "paired calibration")
            elif value == 0x53:
                require(self.volume[channel] == 32 and self.mem[address + 2] == channel * 15, "sampler gain/pan")
                require(self.mem[address + 4] == 1, "sampler address space")
                require(self.be24(address + 5) == self.ring + channel * 2, "sampler start")
                require(self.be24(address + 9) == self.ring_size + 4, "sampler length")
                require(self.be24(address + 17) == 0 and self.be24(address + 21) == self.ring_size, "sampler repeat")
                if channel == 1:
                    require(self.mem[0xdf2e:0xdf30] == self.mem[0xdf4e:0xdf50], "sampler divider pair")
                    require(self.control[0] == 0x53 and self.ticks - self.playback[0]["start"] <= 2,
                            "sampler starts must be adjacent")
                self.playback[channel] = {"start": self.ticks, "played": 0, "position": 0,
                                          "reference": pcm_reference(self.selected[1])}
                self.starts[channel] += 1
            else:
                require(value == 0, f"unexpected sampler control ${value:02x}")
        elif address == 0xdf01:
            self.transfer(value)

    def be24(self, address):
        return int.from_bytes(self.mem[address:address + 3], "big")

    def flush_playback(self):
        """Read PCM using actual start/stride/repeat registers before ring writes."""
        for channel, voice in enumerate(self.playback):
            if voice is None:
                continue
            base = 0xdf20 + channel * 0x20
            divider = int.from_bytes(self.mem[base + 14:base + 16], "big")
            frames = (self.ticks - voice["start"]) * 6250000 // ((divider + 2) * self.bus_hz)
            while voice["played"] < frames:
                if voice["position"] == self.be24(base + 21):
                    voice["position"] = self.be24(base + 17)
                require(voice["position"] < self.be24(base + 9), "sampler terminal length reached")
                at = self.be24(base + 5) + voice["position"]
                require(self.ring <= at and at + 2 <= self.ring + self.ring_size, "sampler read outside ring")
                # A deliberately fatal service gap can exhaust the ring. Keep
                # register/bounds checks but leave stale-audio detection to its
                # explicit underrun assertion.
                expected = next(voice["reference"])[channel]
                if voice["played"] < self.produced:
                    actual = int.from_bytes(self.reu[at:at + 2], "little", signed=True)
                    require(actual == expected, f"sampler channel {channel} frame {voice['played']} differs")
                    self.playback_checked[channel] += 1
                voice["position"] += 4
                voice["played"] += 1

    def transfer(self, command):
        self.flush_playback()
        ram = int.from_bytes(self.mem[0xdf02:0xdf04], "little")
        reu = int.from_bytes(self.mem[0xdf04:0xdf07], "little")
        count = int.from_bytes(self.mem[0xdf07:0xdf09], "little")
        require(1 <= count <= 1024, f"unbounded DMA {count}")
        require(ram + count <= 65536 and reu + count <= len(self.reu), "DMA extent")
        require(self.mem[0xdf09] == self.mem[0xdf0a] == 0, "DMA address controls")
        self.max_dma = max(self.max_dma, count)
        if command == 0x91:
            require(all(self.allowed[ram:ram + count]), f"DMA into unowned RAM ${ram:04x}")
            self.mem[ram:ram + count] = self.reu[reu:reu + count]
            self.fetches += 1
            if reu in self.references and count == 32:
                self.selected = self.references[reu]
        elif command == 0x90:
            require(self.ring <= reu and reu + count <= self.ring + self.ring_size, "REU write outside PCM ring")
            data = self.mem[ram:ram + count]
            offset = reu - self.ring
            self.ring_banks.add(offset >> 16)
            if count == 1024:
                require(not any(data) and self.control == [0, 0], "initial ring clear must be silent")
                if offset == 0:
                    require(self.selected is not None, "ring clear before descriptor load")
                    self.reference = pcm_reference(self.selected[1])
                    self.produced = 0
            else:
                require(count <= 512 and count % 4 == 0, "PCM batch bound")
                require(offset == self.produced * 4 % self.ring_size, "PCM ring position")
                expected = b"".join(struct.pack("<hh", *next(self.reference)) for _ in range(count // 4))
                require(data == expected, f"PCM differs: {self.selected[0]} sample {self.produced}")
                if offset == 0 and self.produced:
                    self.ring_wraps += 1
                self.produced += count // 4
                self.pcm_samples[self.selected[0]] += count // 4
            self.reu[reu:reu + count] = data
            self.stashes += 1
        else:
            raise KernelError(f"unexpected REU command ${command:02x}")
        self.extra_ticks += count

    def call(self, name, budget, expect_fault=False):
        require(self.s == 0xfd, "hardware stack before call")
        stack = bytes(self.mem[2:4])
        self.push(SENTINEL >> 8)
        self.push((SENTINEL - 1) & 255)
        self.pc = self.symbols[name].address
        start, ticks = self.cycles, self.ticks
        samples = sum(self.pcm_samples.values())
        failure = self.symbols["havoc_c64::fail"].address
        for steps in range(budget):
            if self.pc in (SENTINEL, failure):
                break
            self.step()
        else:
            raise KernelError(f"instruction budget exhausted in {name} at ${self.pc:04x}")
        faulted = self.pc == failure
        sample_delta = sum(self.pcm_samples.values()) - samples
        cycle_delta = self.cycles - start
        require(faulted == expect_fault,
                f"{name}: unexpected failure={faulted}, digital fault={self.get('DIGITAL_FAULT')}; "
                f"call {len(self.calls)}, {cycle_delta} CPU cycles, {sample_delta} PCM samples, "
                f"{self.ticks - ticks} bus ticks, total produced {self.produced}")
        if not faulted:
            require(self.s == 0xfd and self.mem[2:4] == stack, "stack imbalance")
        self.calls.append({"entry": name, "instructions": steps, "cpu_cycles": self.cycles - start,
                           "bus_ticks": self.ticks - ticks,
                           "pcm_samples": sum(self.pcm_samples.values()) - samples, "fault": faulted})

    def delay(self, seconds):
        self.extra_ticks += round(seconds * self.bus_hz)
        self.flush_playback()
        # GEOMETRY/PIXELS/SIM can consume all shared staging between AUDIO calls.
        self.mem[0x8e00:0x9300] = bytes((i * 71 + 19) & 255 for i in range(0x500))


def exercise(elf, symbols, image, standard, budget):
    cpu = Machine(elf, symbols, image, standard)
    require(cpu.get("EMULATOR_SID_ONLY") == 0, "production must default to digital audio")
    service = lambda **kw: cpu.call("havoc_c64::service_audio", budget, **kw)
    service()
    require(cpu.get("MUSIC_BACKEND") == 1 and cpu.starts == [1, 1], "DAC startup")
    require(cpu.selected[0] == "hammond-rocker", "startup title must select Hammond Rocker")
    require(cpu.produced == 32768, "full initial ring prefill")
    print(f"{standard}: calibrated and primed Hammond Rocker for title/attract", flush=True)
    audio = symbols["havoc_c64::AUDIO"]
    before = bytes(cpu.mem[audio.address:audio.address + audio.size])
    for _ in range(50):
        cpu.call("havoc_c64::audio_tick", budget)
    require(cpu.mem[audio.address:audio.address + audio.size] == before, "50 Hz IRQ changed DAC state")
    for frame in range(9):
        cpu.delay(0.18)
        service()
        print(f"{standard}: title refill {frame + 1}: {cpu.calls[-1]}", flush=True)
    require(cpu.starts == [1, 1] and cpu.ring_wraps > 0, "title service restarted or never wrapped")
    calibration_count = cpu.calibrations
    cpu.put("havoc_c64::REQUESTED_CUE", 1)
    service()
    require(cpu.get("MUSIC_BACKEND") == 0 and cpu.control == cpu.volume == [0, 0], "DAC to SID transition")
    cpu.call("havoc_c64::audio_tick", budget)
    cpu.put("havoc_c64::REQUESTED_CUE", 0)
    service()
    require(cpu.get("MUSIC_BACKEND") == 1 and cpu.starts == [2, 2], "SID to DAC transition")
    require(cpu.selected[0] == "hammond-rocker", "default menu selection must return to Hammond Rocker")
    require(cpu.calibrations == calibration_count, "saved calibration was discarded")
    calibrated_divider = bytes(cpu.mem[0xdf2e:0xdf30])

    def select_menu_song(cue, song, frames):
        starts = cpu.starts.copy()
        samples = cpu.pcm_samples[song]
        cpu.put("havoc_c64::REQUESTED_CUE", cue)
        service()
        require(cpu.get("MUSIC_BACKEND") == 1 and cpu.selected[0] == song,
                f"menu selection did not choose {song}")
        require(cpu.starts == [count + 1 for count in starts], f"{song} must start once per toggle")
        # Ring writes are checked against a fresh independent PCM decoder, so
        # a full prefill proves the selected song starts from its first sample.
        require(cpu.produced == 32768 and cpu.pcm_samples[song] - samples == 32768,
                f"{song} did not restart with a full ring prefill")
        require(cpu.calibrations == calibration_count, f"{song} discarded saved calibration")
        require(bytes(cpu.mem[0xdf2e:0xdf30]) == calibrated_divider,
                f"{song} changed the calibrated sampler divider")
        starts = cpu.starts.copy()
        playback_starts = [voice["start"] for voice in cpu.playback]
        for _ in range(frames):
            cpu.delay(0.18)
            # The current menu choice is published each frame, and must keep
            # playing until the hidden chord changes the selection again.
            cpu.put("havoc_c64::REQUESTED_CUE", cue)
            service()
            require(cpu.selected[0] == song and cpu.starts == starts,
                    f"repeated title/attract requests restarted or replaced {song}")
            require(cpu.calibrations == calibration_count, "repeated menu requests recalibrated")
        require([voice["start"] for voice in cpu.playback] == playback_starts,
                f"repeated title/attract requests changed {song} sampler position")
        print(f"{standard}: {song} toggled from the beginning and continued for {frames} menu requests",
              flush=True)

    select_menu_song(5, "galactic-dash", 9)
    select_menu_song(0, "hammond-rocker", 3)
    select_menu_song(5, "galactic-dash", 3)
    cpu.delay(1.05)
    service()
    require(cpu.get("DIGITAL_MINIMUM_LEAD", 2) > 2048, "below-guard delay depleted ring")
    cpu.delay(0.18)
    service()
    require(cpu.get("DIGITAL_FAULT") == 0, "bounded catch-up did not recover on the following frame")
    print(f"{standard}: both tracks, scratch reuse, ring wraps and SID handoff passed", flush=True)
    cpu.delay(1.40)
    service(expect_fault=True)
    require(cpu.get("DIGITAL_FAULT") == 5 and cpu.control == cpu.volume == [0, 0], "underrun did not fault and mute")
    require(cpu.ring_banks == {0, 1}, "PCM DMA never reached both ring banks")
    require(min(cpu.playback_checked) > 100_000, "both hardware channel read paths must be exercised")
    return {"pcm_samples_checked": cpu.pcm_samples, "starts": cpu.starts,
            "sampler_channel_frames_checked": cpu.playback_checked,
            "calibration_shots": cpu.calibrations, "ring_wraps": cpu.ring_wraps,
            "fetches": cpu.fetches, "stashes": cpu.stashes, "max_dma_bytes": cpu.max_dma,
            "minimum_lead_samples": cpu.get("DIGITAL_MINIMUM_LEAD", 2),
            "main_stack_bytes_written": cpu.stack_top - cpu.stack_min,
            "fault": cpu.get("DIGITAL_FAULT"), "calls": cpu.calls}


def exercise_emulator_sid_only(elf, symbols, image, standard, budget):
    strict = Machine(elf, symbols, image, standard, sampler_present=False)
    require(strict.get("EMULATOR_SID_ONLY") == 0, "production must not opt into emulator mode")
    strict.call("havoc_c64::service_audio", budget, expect_fault=True)
    require(strict.get("DIGITAL_FAULT") == 1, "missing production sampler must report Mapping")
    require(strict.get("MUSIC_BACKEND") != 0, "missing production sampler silently fell back to SID")
    print(f"{standard}: missing sampler still fails in production mode", flush=True)

    cues = {}
    for cue, name in ((0, "title"), (5, "victory")):
        cpu = Machine(elf, symbols, image, standard, sampler_present=False)
        # Match monitor setup after the CRT has initialized globals, before
        # the first AUDIO call. Each session starts with uninitialized AUDIO.
        cpu.put("EMULATOR_SID_ONLY", 1)
        cpu.put("havoc_c64::REQUESTED_CUE", cue)
        cpu.call("havoc_c64::service_audio", budget)
        require(cpu.get("MUSIC_BACKEND") == 0 and cpu.get("MUSIC_VALIDATED") == 1,
                f"emulator {name} did not initialize the SID backend")
        gated_voices = set()
        for _ in range(50):
            cpu.call("havoc_c64::audio_tick", budget)
            cpu.call("havoc_c64::service_audio", budget)
            for sid in range(4):
                for voice in range(3):
                    base = 0xd400 + sid * 32 + voice * 7
                    if cpu.mem[base + 4] & 1 and any(cpu.mem[base:base + 2]):
                        gated_voices.add((sid, voice))
        require(cpu.get("MUSIC_CUE") == cue and cpu.get("MUSIC_PAGE") != 0xff,
                f"emulator {name} did not consume its SID score")
        require(gated_voices, f"emulator {name} never played a SID note")
        require(cpu.get("MUSIC_UNDERRUNS", 2) == 0 and cpu.get("DIGITAL_FAULT") == 0,
                f"emulator {name} reported an audio fault")
        require(cpu.sampler_reads == cpu.sampler_writes == 0,
                f"emulator {name} accessed unavailable sampler hardware")
        cues[name] = {"cue": cpu.get("MUSIC_CUE"), "page": cpu.get("MUSIC_PAGE"),
                     "gated_sid_voices": len(gated_voices), "audio_ticks": 50,
                     "sampler_reads": cpu.sampler_reads, "sampler_writes": cpu.sampler_writes,
                     "fault": cpu.get("DIGITAL_FAULT")}
        print(f"{standard}: emulator SID-only {name} played without a sampler", flush=True)
    return {"sampler_present": False, "strict_production_fault": strict.get("DIGITAL_FAULT"),
            "emulator_flag": 1, "cues": cues, "passed": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", type=Path, default=ROOT / "dist/havoc.elf")
    parser.add_argument("--reu", type=Path, default=ROOT / "dist/havoc.reu")
    parser.add_argument("--standard", choices=("pal", "ntsc", "both"), default="both")
    parser.add_argument("--budget", type=int, default=100_000_000)
    parser.add_argument("--emulator-only", action="store_true",
                        help="only verify explicit SID-only playback and missing-sampler rejection")
    parser.add_argument("--report", type=Path, default=ROOT / "target/digital-kernel/report.json")
    args = parser.parse_args()
    elf, symbols = load_elf(args.elf)
    image = args.reu.read_bytes()
    require(len(image) == symbols["__reu_size"].address, "REU/ELF size mismatch; rebuild both")
    report = {"model": "NMOS 6502 at 64x bus; DMA 1 byte/bus cycle; stereo sampler 6.25 MHz/(divider+2)",
              "digital_playback_tested": not args.emulator_only,
              "elf_sha256": hashlib.sha256(args.elf.read_bytes()).hexdigest(),
              "reu_sha256": hashlib.sha256(image).hexdigest(), "standards": {}}
    for standard in ("pal", "ntsc") if args.standard == "both" else (args.standard,):
        result = {} if args.emulator_only else exercise(elf, symbols, image, standard, args.budget)
        result["emulator_sid_only"] = exercise_emulator_sid_only(elf, symbols, image, standard, args.budget)
        report["standards"][standard] = result
    report["passed"] = True
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    label = "emulator SID-only" if args.emulator_only else "digital and emulator SID-only"
    print(f"native {label} integration passed; {args.report}")


if __name__ == "__main__":
    main()
