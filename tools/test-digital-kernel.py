#!/usr/bin/env python3
"""Execute linked DLT4 decoding, BOOT, and AUDIO against independent models.

This is modeled NMOS6502 execution at 64x PAL/NTSC bus speed, with DMA charged
one bus tick/byte. The sampler clock is 6.25MHz/(divider+2); the extra two ticks
are synthetic calibration stress, not a physical hardware measurement.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import re
import runpy
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
KERNEL = runpy.run_path(str(ROOT / "tools/native_cpu.py"))
CATALOG = runpy.run_path(str(ROOT / "tools/digital_format.py"))
Cpu, KernelError, load_elf = (KERNEL[name] for name in ("Cpu", "KernelError", "load_elf"))
SENTINEL = 0xffff
FRAMES = 32768
DELAY = 160


def require(condition, message):
    if not condition:
        raise KernelError(message)


def read_digital_layout(elf, dwarf):
    """Read fade/clock state from this linked target rather than fixed offsets."""
    members = runpy.run_path(str(ROOT / "tools/vice-smoke.py"))["dwarf_members"]
    result = {}
    for label in ("State", "Decoder", "SampleClock"):
        description = subprocess.check_output(
            [str(dwarf), f"--name={label}", "--show-children", str(elf)], text=True)
        sizes = {int(value, 0) for value in re.findall(
            rf'DW_TAG_structure_type\s+DW_AT_name\s+\("{label}"\)\s+DW_AT_byte_size\s+\((0x[0-9a-fA-F]+|\d+)\)',
            description)}
        require(len(sizes) == 1, f"missing/conflicting digital DWARF size: {label}")
        result[label] = dict(size=sizes.pop(), fields=members(description, label))
    fields = result["State"]["fields"]
    for field, kind in (("fade_tick", "u32"), ("fade_step", "u16"), ("volume", "u8"),
                        ("active", "u8"), ("pending", "u8"), ("last_tick", "u32"), ("fading", "bool")):
        require(fields[field]["type"] == kind, f"digital state ABI: {field}")
    require(result["Decoder"]["fields"]["produced"]["type"] == "u32"
            and result["SampleClock"]["fields"]["samples"]["type"] == "u32",
            "digital sample counter ABI")
    return result


def steady_aggregates(calls, bus_hz):
    """Weight work by frames and audio duration, excluding primes and switches."""
    tracks = {}
    for call in calls:
        if not call["steady"] or call["fault"] or not call["pcm_samples"]:
            continue
        result = tracks.setdefault(call["active_track"], dict(
            calls=0, frames=0, cpu_cycles=0, bus_ticks=0, audio_seconds=0.0))
        result["calls"] += 1
        result["frames"] += call["pcm_samples"]
        result["cpu_cycles"] += call["cpu_cycles"]
        result["bus_ticks"] += call["bus_ticks"]
        result["audio_seconds"] += call["pcm_samples"] / call["modeled_sampler_rate_hz"]
    for result in tracks.values():
        frames, seconds = result["frames"], result["audio_seconds"]
        wall_us = result["bus_ticks"] * 1000000 / bus_hz
        result.update(cpu_cycles_per_frame=result["cpu_cycles"] / frames,
                      bus_ticks_per_frame=result["bus_ticks"] / frames,
                      wall_us_per_frame=wall_us / frames,
                      modeled_sampler_rate_hz=frames / seconds,
                      estimated_service_us_per_audio_second=wall_us / seconds)
    return tracks


def pcm_reference(blob):
    """Independent bit-at-a-time DLT4 decoder yielding signed8 frame tuples."""
    samples = int.from_bytes(blob[8:12], "little")
    books = int.from_bytes(blob[18:20], "little")
    channels = blob[21]
    require(blob[:4] == b"DLT4" and blob[20] == 3 and channels in (1, 2), "DLT4 profile")
    alignment = 8 // channels
    segment = samples if books == 1 else samples // books // alignment * alignment
    stream = 32 + books * 24 * channels
    while True:
        accumulator = [0] * channels
        for frame in range(samples):
            book = min(frame // segment, books - 1)
            output = []
            for channel in range(channels):
                bit = (frame * channels + channel) * 3
                code = sum(((blob[stream + (bit + shift) // 8] >> ((bit + shift) % 8)) & 1) << shift
                           for shift in range(3))
                entry = 32 + (book * channels * 8 + channel * 8 + code) * 3
                magnitude = int.from_bytes(blob[entry:entry + 2], "little")
                delta = -min(magnitude, 32768) if blob[entry + 2] else min(magnitude, 32767)
                accumulator[channel] = min(32767, max(-32768, accumulator[channel] + delta))
                output.append(min(127, max(-128, (accumulator[channel] + 128) // 256)))
            yield tuple(output)


def exercise_decode_kernel(elf, symbols):
    """Adversarial ABI cases, independent of the Rust driver and asset encoder."""
    sections = {section.name: section for section in elf.sections}
    base = bytearray(65536)
    for name in (*KERNEL["RESIDENT_SECTIONS"], ".phase.audio"):
        section = sections[name]
        base[section.address:section.address + section.size] = elf.contents(section)
    kernel, source, output = 0x8e00, 0x9000, 0x9100
    randomizer = random.Random(0xD174)
    cases, cycles, frames_total = 0, 0, 0
    profiles = {channels: dict(cpu_cycles=0, frames=0) for channels in (1, 2)}
    for channels in (1, 2):
        for valid_bits in range(8):
            for count in (1, 2, 3, 7, 8, 31, 42, 85, 127, 128):
                memory = bytearray(base)
                book = [-32768, -30000, -257, -128, 0, 127, 256, 32767] * 2
                randomizer.shuffle(book)
                initial = [randomizer.choice((-32768, -32767, -129, -128, 127, 128, 32640, 32767)) for _ in range(2)]
                reservoir = randomizer.randrange(1 << valid_bits)
                data = bytes(randomizer.randrange(256) for _ in range(128))
                struct.pack_into("<16h2hHBB", memory, kernel, *book, *initial, reservoir, valid_bits, channels)
                memory[source:source + len(data)] = data
                memory[output - 1:output + 257] = bytes([0xa5]) * 258
                memory[16:32] = bytes(range(0xc0, 0xd0))
                cpu = Cpu(memory, [(0, 0x200), (kernel + 32, kernel + 40), (output, output + count * channels)])
                # LLVM-MOS allocates the byte argument in A before allocating
                # pointer arguments to register pairs. Match the real Rust call.
                cpu.a, cpu.x = count, 0x71
                memory[4:6] = kernel.to_bytes(2, "little")  # __rc2 / __rc3
                memory[6:8] = source.to_bytes(2, "little")  # __rc4 / __rc5
                memory[8:10] = output.to_bytes(2, "little") # __rc6 / __rc7
                preserved = bytes(memory[18:34])  # ABI callee-saved __rc16..31
                before_book = bytes(memory[kernel:kernel + 32])
                accumulator, bits, packed, used = initial.copy(), valid_bits, reservoir, 0
                expected = bytearray()
                for _ in range(count):
                    for channel in range(channels):
                        if bits < 3:
                            packed |= data[used] << bits
                            used += 1
                            bits += 8
                        code, packed, bits = packed & 7, packed >> 3, bits - 3
                        accumulator[channel] = min(32767, max(-32768, accumulator[channel] + book[channel * 8 + code]))
                        expected.append(min(127, max(-128, (accumulator[channel] + 128) // 256)) & 255)
                cpu.push(SENTINEL >> 8)
                cpu.push((SENTINEL - 1) & 255)
                cpu.pc = symbols["digital_decode_run"].address
                for _ in range(100000):
                    if cpu.pc == SENTINEL:
                        break
                    cpu.step()
                else:
                    raise KernelError("digital_decode_run exceeded instruction bound")
                require(cpu.s == 0xfd and cpu.a == used, "decode kernel return/stack ABI")
                require(memory[output:output + len(expected)] == expected,
                        f"decode kernel PCM channels={channels} bits={valid_bits} count={count}")
                require(struct.unpack_from("<2hHBB", memory, kernel + 32) == (*accumulator, packed, bits, channels),
                        "decode kernel predictor/reservoir state")
                require(memory[kernel:kernel + 32] == before_book and memory[source:source + len(data)] == data,
                        "decode kernel modified immutable inputs")
                require(memory[18:34] == preserved and memory[output - 1] == memory[output + len(expected)] == 0xa5,
                        "decode kernel ABI or output guard")
                cases += 1
                cycles += cpu.cycles
                frames_total += count
                profiles[channels]["cpu_cycles"] += cpu.cycles
                profiles[channels]["frames"] += count
    for result in profiles.values():
        result["average_cycles_per_frame"] = result["cpu_cycles"] / result["frames"]
    return dict(cases=cases, cpu_cycles=cycles, frames=frames_total,
                average_cycles_per_frame=cycles / frames_total, profiles=profiles)


class Machine(Cpu):
    def __init__(self, elf, symbols, image, standard, *, sampler_present=True):
        self.elf, self.symbols = elf, symbols
        self.sections = {s.name: s for s in elf.sections}
        memory = bytearray(65536)
        for name in KERNEL["RESIDENT_SECTIONS"]:
            section = self.sections[name]
            memory[section.address:section.address + section.size] = elf.contents(section)
        writable = [(0, 0x200), (0x8e00, 0x9300), (0xd400, 0xd480), (0xdd00, 0xdd10), (0xdf00, 0xdf60)]
        for name in (".data", ".bss", ".noinit", ".audio_state", ".game_state", ".sfx_state"):
            section = self.sections.get(name)
            if section:
                writable.append((section.address, section.address + section.size))
        for name in ("havoc_c64::AUDIO", "havoc_c64::EFFECTS", "havoc_c64::GAME", "havoc_c64::ERROR_DIGITAL"):
            symbol = symbols[name]
            writable.append((symbol.address, symbol.address + symbol.size))
        writable += [(symbols["__stack_bottom"].address, symbols["__stack"].address),
                     (symbols["__irq_stack_bottom"].address, symbols["__irq_stack"].address)]
        super().__init__(memory, writable)
        self.allowed = bytearray(65536)
        for begin, end in writable:
            self.allowed[begin:end] = b"\1" * (end - begin)
        self.reu = bytearray(image)
        self.ring = symbols["__reu_digital_ring"].address
        self.ring_size = symbols["__reu_digital_ring_size"].address
        self.bus_hz = 985248 if standard == "pal" else 1022727
        self.extra_ticks = self.timer_origin = 0
        self.deadline = None
        self.sampler_present = sampler_present
        self.sampler_reads = self.sampler_writes = 0
        self.control, self.volume, self.starts = [0, 0], [0, 0], [0, 0]
        self.volume_writes, self.voice_starts = [], []
        self.low_bytes = [0xa5, 0x5a]  # 8-bit reads must not inherit these.
        self.calibrations = self.wide_clears = 0
        self.playback, self.playback_checked = [None, None], [0, 0]
        self.fetches = self.stashes = self.max_dma = self.ring_wraps = self.prefix_clears = 0
        self.ring_banks = set()
        self.catalog = CATALOG["load_catalog"](validate=False)
        self.references = self.catalog["tracks"]
        self.last_source_track = None
        self.rings = [None, None]
        self.pcm_samples = {entry["name"]: 0 for entry in self.references}
        self.calls = []
        self.reported_profiles = set()
        self.first_kernel_call = None
        self.allow_corrupt_pcm = False
        self.put("MUSIC_BACKEND", 2)
        self.put("havoc_c64::REQUESTED_CUE", 0)
        self.put("havoc_c64::AUDIO_READY", 0)
        self.put("havoc_c64::CLOCK_KHZ", 985 if standard == "pal" else 1023, 2)
        self.stack_bottom = symbols["__stack_bottom"].address
        self.stack_top = self.stack_min = symbols["__stack"].address
        self.mem[2:4] = self.stack_top.to_bytes(2, "little")
        self.load_phase("boot")

    @property
    def ticks(self):
        return self.cycles // 64 + self.extra_ticks

    def put(self, name, value, length=1):
        symbol = self.symbols.get(name) or self.symbols[name.rsplit("::", 1)[-1]]
        self.mem[symbol.address:symbol.address + length] = value.to_bytes(length, "little")

    def get(self, name, length=1):
        symbol = self.symbols.get(name) or self.symbols[name.rsplit("::", 1)[-1]]
        return int.from_bytes(self.mem[symbol.address:symbol.address + length], "little")

    def state_address(self, field):
        return self.symbols["havoc_c64::AUDIO"].address + self.elf.digital_layout["State"]["fields"][field]["offset"]

    def state_value(self, field):
        kind = self.elf.digital_layout["State"]["fields"][field]["type"]
        length = {"u8": 1, "u16": 2, "u32": 4, "bool": 1}[kind]
        address = self.state_address(field)
        return int.from_bytes(self.mem[address:address + length], "little")

    def rebase_sample_counters(self):
        """Force the next advance through a whole-ring rebase without a long run."""
        layout = self.elf.digital_layout
        decoder = self.state_address("decoders") + self.state_value("active") * layout["Decoder"]["size"]
        addresses = (decoder + layout["Decoder"]["fields"]["produced"]["offset"],
                     self.state_address("clock") + layout["SampleClock"]["fields"]["samples"]["offset"])
        for address in addresses:
            value = int.from_bytes(self.mem[address:address + 4], "little")
            self.mem[address:address + 4] = (value + 0x01000000).to_bytes(4, "little")
        return addresses[1]

    def pending_frames(self):
        slot = self.state_value("pending")
        if slot == 255:
            return None
        decoder = self.elf.digital_layout["Decoder"]
        address = self.state_address("decoders") + slot * decoder["size"] + decoder["fields"]["produced"]["offset"]
        return int.from_bytes(self.mem[address:address + 4], "little")

    def wrap_timer_soon(self):
        """Move the CIA epoch, preserving driver intervals and sampler time."""
        counter = (0xffffffff - (self.ticks - self.timer_origin)) & 0xffffffff
        shift = 128 - counter
        self.timer_origin += shift
        for field in ("last_tick", "fade_tick"):
            address = self.state_address(field)
            value = (self.state_value(field) + shift) & 0xffffffff
            self.mem[address:address + 4] = value.to_bytes(4, "little")

    def requested_track(self):
        cue = self.get("havoc_c64::REQUESTED_CUE")
        stage = self.get("havoc_c64::REQUESTED_STAGE")
        # Independent scene oracle: menu/result/ascent cues always win. The
        # stage map distinguishes recordings whose original SID cues coincide.
        track = self.catalog["cues"][cue]
        if cue not in (0, 5, 6, 19) and stage < len(self.catalog["stages"]):
            override = self.catalog["stages"][stage]
            if override != 255:
                track = override
        return self.references[track]["name"]

    def load_phase(self, name):
        section = self.sections[f".phase.{name}"]
        self.mem[section.address:section.address + section.size] = self.elf.contents(section)
        self.phase = name

    def boot(self, budget, expect_fault=False):
        self.load_phase("boot")
        self.call("havoc_c64::initialize_music_boot", budget, expect_fault=expect_fault)
        if not expect_fault:
            self.load_phase("audio")

    def read(self, address):
        address &= 0xffff
        if 0xdd04 <= address <= 0xdd07:
            timer = (0xffffffff - (self.ticks - self.timer_origin)) & 0xffffffff
            return (timer >> (8 * (address - 0xdd04))) & 255
        if address == 0xdf21:
            self.sampler_reads += 1
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
            channel = (address - 0xdf21) // 0x20
            voice = self.playback[channel]
            self.volume[channel] = value
            self.volume_writes.append(dict(tick=self.ticks, channel=channel, value=value,
                call=len(self.calls),
                track=None if voice is None else self.rings[voice["slot"]]["track"]["name"]))
        elif address in (0xdf20, 0xdf40):
            channel = (address - 0xdf20) // 0x20
            self.control[channel] = value
            self.playback[channel] = None
            if channel == 0:
                self.deadline = None
            if value in (0x55, 0x45, 0x05):
                require(channel == 0, "left calibration requests completion IRQ")
                stride = 4 if value == 0x55 else 2 if value == 0x45 else 1
                require(self.be24(address + 21) == 0, "one-shot repeat B must be zero")
                frames = self.be24(address + 9) // stride
                divider = self.be16(address + 14)
                self.deadline = self.ticks + round(frames * self.bus_hz * (divider + 2) / 6250000)
                self.calibrations += 1
                if value == 0x55:
                    require(self.reu[self.be24(address + 5)] == 0, "wide clearing sample must be zero")
                    self.low_bytes[0] = 0
                    self.wide_clears += 1
            elif value in (0x51, 0x41, 0x01):
                require(channel == 1 and self.control[0] == value | 4, "paired calibration")
                if value == 0x51:
                    require(self.reu[self.be24(address + 5)] == 0, "wide right clearing sample")
                    self.low_bytes[1] = 0
            elif value in (0x43, 0x03):
                self.start_voice(channel, address, value)
            else:
                require(value == 0, f"unexpected sampler control ${value:02x}")
        elif address == 0xdf01:
            self.transfer(value)
        if address in (0xdf20, 0xdf40, 0xdf21, 0xdf41):
            audible_slots = {voice["slot"] for channel, voice in enumerate(self.playback)
                             if voice is not None and self.volume[channel] != 0}
            require(len(audible_slots) <= 1, "two source tracks were audible simultaneously")

    def start_voice(self, channel, address, control):
        channels = 2 if control == 0x43 else 1
        start, repeat_a, repeat_b = (self.be24(address + offset) for offset in (5, 17, 21))
        delay = DELAY if channels == 1 and channel else 0
        ring_base = start + delay - (channel if channels == 2 else 0)
        slot = (ring_base - self.ring) >> 16
        require(slot in (0, 1), "sampler slot")
        require(ring_base == self.ring + slot * 65536 + (32768 if channels == 1 else 0), "sampler ring base")
        require(repeat_a == delay and repeat_b == FRAMES * channels + delay, "sampler repeat boundaries")
        require(self.be24(address + 9) == repeat_b + channels, "sampler terminal boundary")
        require(self.volume[channel] == 32 and self.mem[address + 2] == channel * 15, "sampler gain/pan")
        require(self.mem[address + 4] == 1 and self.low_bytes[channel] == 0, "sampler space or stale low byte")
        ring = self.rings[slot]
        require(ring and ring["channels"] == channels and ring["produced"] == FRAMES, "handoff before full priming")
        if channel:
            require(self.playback[0] and self.playback[0]["slot"] == slot and self.control[0] == control,
                    "sampler voice pair")
            require(self.be16(0xdf2e) == self.be16(0xdf4e), "sampler divider pair")
            require(self.ticks - self.playback[0]["start"] <= 2, "sampler starts must be adjacent")
        self.playback[channel] = dict(start=self.ticks, played=0, position=0, slot=slot, channels=channels,
                                      delay=delay, reference=pcm_reference(ring["track"]["blob"]))
        self.starts[channel] += 1
        self.voice_starts.append(dict(tick=self.ticks, channel=channel,
                                     track=ring["track"]["name"], slot=slot, volume=self.volume[channel], call=len(self.calls)))

    def be16(self, address):
        return int.from_bytes(self.mem[address:address + 2], "big")

    def be24(self, address):
        return int.from_bytes(self.mem[address:address + 3], "big")

    def active_name(self):
        voice = self.playback[0]
        return None if voice is None else self.rings[voice["slot"]]["track"]["name"]

    def queue_death_effect(self, budget):
        # Previous probes intentionally advanced effects only once per service,
        # so retire their old tail before checking a fresh two-SID instance.
        for _ in range(max(self.effects_remaining(), default=0) + 1):
            self.call("havoc_c64::audio_tick", budget)
        effects = self.symbols["EFFECTS"]
        layout = self.elf.effects_layout
        engine, event = layout["EffectsEngine"], layout["AudioEvent"]
        require(effects.size == engine["size"], "effects fixture ABI")
        entry = effects.address + engine["fields"]["events"]["offset"]
        self.mem[entry + event["fields"]["effect"]["offset"]] = 16
        self.mem[entry + event["fields"]["pan"]["offset"]] = 0
        self.mem[effects.address + engine["fields"]["event_count"]["offset"]] = 1
        self.call("havoc_c64::audio_tick", budget)
        initial = self.effects_remaining()
        self.call("havoc_c64::audio_tick", budget)
        self.death_remaining = initial[0] - 1
        require(initial[0] > 30 and initial == [initial[0]] * 2
                and self.effects_remaining() == [self.death_remaining] * 2,
                "digital playback did not start one stereo SID pair")
        gated = {sid for sid in range(4)
                 if all(self.mem[0xd400 + sid * 32 + voice * 7 + 4] & 1 for voice in range(3))}
        require(len(gated) == 2 and {sid & 1 for sid in gated} == {0, 1}, "stereo effect SID gates")
        return effects.address

    def effects_remaining(self):
        layout = self.elf.effects_layout
        base = self.symbols["EFFECTS"].address + layout["EffectsEngine"]["fields"]["instances"]["offset"]
        instance = layout["Instance"]
        return [self.mem[base + sid * instance["size"] + instance["fields"]["remaining"]["offset"]]
                for sid in range(4)
                if self.mem[base + sid * instance["size"] + instance["fields"]["mask"]["offset"]] != 0]

    def flush_playback(self):
        for channel, voice in enumerate(self.playback):
            if voice is None:
                continue
            base = 0xdf20 + channel * 0x20
            frames = (self.ticks - voice["start"]) * 6250000 // ((self.be16(base + 14) + 2) * self.bus_hz)
            ring = self.rings[voice["slot"]]
            while voice["played"] < frames:
                if voice["position"] == self.be24(base + 21):
                    voice["position"] = self.be24(base + 17)
                require(voice["position"] < self.be24(base + 9), "sampler terminal length reached")
                at = self.be24(base + 5) + voice["position"]
                require(self.ring <= at < self.ring + self.ring_size, "sampler fetch outside REU ring")
                source_frame = voice["played"] - voice["delay"]
                expected = 0 if source_frame < 0 else next(voice["reference"])[channel if voice["channels"] == 2 else 0]
                if source_frame < ring["produced"]:
                    actual = int.from_bytes(bytes((self.low_bytes[channel], self.reu[at])), "little", signed=True)
                    require(actual == expected * 256,
                            f"sampler {ring['track']['name']} channel {channel} frame {voice['played']} differs: {actual} != {expected * 256}")
                    self.playback_checked[channel] += 1
                voice["position"] += voice["channels"]
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
            if self.phase == "audio":
                for entry in self.references:
                    if entry["offset"] <= reu < entry["end"]:
                        self.last_source_track = entry
                        break
        elif command == 0x90:
            require(self.ring <= reu and reu + count <= self.ring + self.ring_size, "write outside mutable PCM arena")
            data = self.mem[ram:ram + count]
            slot, offset = divmod(reu - self.ring, 65536)
            self.ring_banks.add(slot)
            if self.phase == "boot":
                require(not any(data) and self.control == [0, 0], "boot ring clear must be silent")
            elif offset == 32768 - DELAY and count == DELAY:
                require(not any(data), "mono delay prefix was not cleared on preparation")
                require(all(v is None or v["slot"] != slot for v in self.playback), "prefix changed under active reader")
                self.prefix_clears += 1
            else:
                entry = self.last_source_track
                require(entry is not None, "PCM publication without source fetch")
                channels = entry["metadata"]["channels"]
                base = 32768 if channels == 1 else 0
                active = any(v is not None and v["slot"] == slot for v in self.playback)
                if offset == base and not active:
                    self.rings[slot] = dict(track=entry, channels=channels, produced=0, reference=pcm_reference(entry["blob"]))
                ring = self.rings[slot]
                require(ring is not None and ring["track"]["name"] == entry["name"], "PCM source/slot ownership")
                require(0 < count <= 128 * channels and count % channels == 0, "PCM batch bound")
                require(offset == base + ring["produced"] % FRAMES * channels, "PCM ring cursor")
                expected = bytes(sample & 255 for _ in range(count // channels) for sample in next(ring["reference"]))
                if data != expected and not self.allow_corrupt_pcm:
                    mismatch = next(i for i, (a, b) in enumerate(zip(data, expected)) if a != b)
                    raise KernelError(f"PCM differs: {entry['name']} frame {ring['produced']} byte {mismatch}; "
                                      f"actual={data[max(0,mismatch-4):mismatch+12].hex()} expected={expected[max(0,mismatch-4):mismatch+12].hex()}; "
                                      f"first_kernel_call={self.first_kernel_call}")
                if offset == base and ring["produced"]:
                    self.ring_wraps += 1
                ring["produced"] += count // channels
                self.pcm_samples[entry["name"]] += count // channels
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
        start, ticks, samples = self.cycles, self.ticks, sum(self.pcm_samples.values())
        samples_by_track = self.pcm_samples.copy()
        active = self.active_name()
        starts = self.starts.copy()
        divider = self.be16(0xdf2e) if active is not None else None
        requested = self.requested_track()
        steady = name.endswith("service_audio") and active is not None and active == requested
        failure = self.symbols["havoc_c64::fail"].address
        for steps in range(budget):
            if self.pc in (SENTINEL, failure):
                break
            if self.first_kernel_call is None and self.pc == self.symbols["digital_decode_run"].address:
                pointer = int.from_bytes(self.mem[4:6], "little")
                source = int.from_bytes(self.mem[6:8], "little")
                self.first_kernel_call = dict(pointer=hex(pointer), state=self.mem[pointer:pointer+40].hex(),
                    source=hex(source), input=self.mem[source:source+16].hex(),
                    output=hex(int.from_bytes(self.mem[8:10], "little")), count=self.a,
                    registers=self.mem[2:18].hex())
            self.step()
        else:
            raise KernelError(f"instruction budget exhausted in {name} at ${self.pc:04x}")
        faulted = self.pc == failure
        require(faulted == expect_fault,
                f"{name}: failure={faulted}, digital fault={self.get('DIGITAL_FAULT')}, call={len(self.calls)}")
        if not faulted:
            require(self.s == 0xfd and self.mem[2:4] == stack, "stack imbalance")
        result = dict(entry=name, instructions=steps, cpu_cycles=self.cycles - start,
                      bus_ticks=self.ticks - ticks, pcm_samples=sum(self.pcm_samples.values()) - samples,
                      active_track=active,
                      sampler_divider=divider,
                      modeled_sampler_rate_hz=None if divider is None else 6250000 / (divider + 2),
                      steady=steady and not faulted and self.active_name() == active and self.starts == starts,
                      fault=faulted)
        decoded = {track: count - samples_by_track[track] for track, count in self.pcm_samples.items()
                   if count != samples_by_track[track]}
        units = sum(decoded.get(entry["name"], 0) * (80000 // entry["metadata"]["sample_rate"])
                    for entry in self.references)
        result.update(decoded_frames_by_track=decoded, decoded_audio_ms=units / 80,
                      startup_priming=name.endswith("service_audio") and active is None and units != 0)
        if name.endswith("service_audio") and active is not None:
            require(units <= 20000, f"running audio service decoded more than 250 ms: {units / 80}")
        self.calls.append(result)
        if result["steady"] and result["pcm_samples"] and active not in self.reported_profiles:
            elapsed = result["bus_ticks"] / self.bus_hz
            print(f"steady {active}: {result['cpu_cycles'] / result['pcm_samples']:.1f} CPU cycles/frame, "
                  f"{elapsed * 1000000 / result['pcm_samples']:.3f} modeled us/frame", flush=True)
            self.reported_profiles.add(active)

    def delay(self, seconds):
        self.extra_ticks += round(seconds * self.bus_hz)
        self.flush_playback()
        self.mem[0x8e00:0x9300] = bytes((i * 71 + 19) & 255 for i in range(0x500))


def exercise(elf, symbols, image, standard, budget):
    cpu = Machine(elf, symbols, image, standard)
    cpu.boot(budget)
    service = lambda **kw: cpu.call("havoc_c64::service_audio", budget, **kw)
    service()
    require(cpu.get("MUSIC_BACKEND") == 1 and cpu.starts == [1, 1], "DAC startup")
    require(cpu.active_name() == "hammond-rocker" and cpu.wide_clears > 0, "stereo title and cleared sampler")
    require(cpu.volume == [32, 32] and cpu.state_value("volume") == 32, "initial playback must start at full gain")
    calibration_count = cpu.calibrations
    audio = symbols["havoc_c64::AUDIO"]
    before = bytes(cpu.mem[audio.address:audio.address + audio.size])
    for _ in range(50):
        cpu.call("havoc_c64::audio_tick", budget)
    require(cpu.mem[audio.address:audio.address + audio.size] == before, "effects IRQ changed decoder state")

    def play(visits):
        for _ in range(visits):
            cpu.delay(0.18)
            service()
            cpu.call("havoc_c64::audio_tick", budget)

    fades = []

    def paired_gains(entries, track):
        writes = [entry for entry in entries if entry["track"] == track]
        require(len(writes) % 2 == 0, "unpaired sampler gain write")
        pairs = []
        for left, right in zip(writes[::2], writes[1::2]):
            require(left["channel"] == 0 and right["channel"] == 1
                    and left["value"] == right["value"] and 0 <= left["value"] <= 32
                    and 0 <= right["tick"] - left["tick"] <= 2,
                    "left/right music fade gains differ")
            pairs.append(left)
        return pairs

    def select(cue, stage, expected, cadence=0.1):
        old_name, starts = cpu.active_name(), cpu.starts.copy()
        effects = cpu.queue_death_effect(budget)
        cpu.put("havoc_c64::REQUESTED_CUE", cue)
        cpu.put("havoc_c64::REQUESTED_STAGE", stage)
        require(cpu.requested_track() == expected, "fixture scene-to-track mapping")
        history = len(cpu.volume_writes)
        voice_history = len(cpu.voice_starts)
        fade_began = None
        prime_visits = 0
        for visit in range(70):
            cpu.delay(cadence)
            if visit == 0:
                began = cpu.ticks
            service()
            cpu.call("havoc_c64::audio_tick", budget)
            require(cpu.effects_remaining() == [cpu.death_remaining - 1 - visit] * 2,
                    "cue preparation or handoff reset/stalled SID effects")
            if cpu.active_name() == expected:
                break
            require(cpu.active_name() == old_name, "pending preparation interrupted active music")
            require(cpu.volume[0] == cpu.volume[1] == cpu.state_value("volume"), "fade state/hardware gain differs")
            if not cpu.state_value("fading"):
                require(cpu.pending_frames() < FRAMES and cpu.volume == [32, 32],
                        "old track faded before replacement was fully primed")
                prime_visits += 1
            else:
                require(cpu.pending_frames() == FRAMES, "normal fade began before full priming")
                if fade_began is None:
                    require(cpu.volume == [32, 32], "fade start skipped full volume")
                    counter = (0xffffffff - (cpu.ticks - cpu.timer_origin)) & 0xffffffff
                    fade_began = cpu.ticks - ((cpu.state_value("fade_tick") - counter) & 0xffffffff)
        else:
            raise KernelError(f"pending track never completed: {expected}")
        require(cpu.starts == [value + 1 for value in starts], "bounded single handoff")
        require(cpu.calibrations == calibration_count, "transition recalibrated")
        gains = paired_gains(cpu.volume_writes[history:], old_name)
        levels = [entry["value"] for entry in gains]
        require(levels and levels[-1] == 0 and levels == sorted(levels, reverse=True)
                and len(set(value for value in levels if 0 < value < 32)) >= 4,
                "music did not fade monotonically through intermediate gains")
        zero = next(entry for entry in gains if entry["value"] == 0)
        zero_at = zero["tick"]
        require(fade_began is not None and prime_visits > 0, "prime and fade phases were not both observed")
        duration = (zero_at - fade_began) / cpu.bus_hz
        require(0.99 <= duration <= 1.25, f"fade duration is not about one second: {duration}")
        first_start = cpu.voice_starts[voice_history]
        require(first_start["tick"] > zero_at and first_start["call"] == zero["call"]
                and cpu.volume == [32, 32],
                "new track started before silence or below full gain")
        fades.append(dict(previous=old_name, next=expected, fade_seconds=duration,
                          prime_seconds=(fade_began - began) / cpu.bus_hz,
                          silence_seconds=(first_start["tick"] - zero_at) / cpu.bus_hz,
                          handoff_seconds=(first_start["tick"] - began) / cpu.bus_hz,
                          gain_steps=levels, channels_matched=True,
                          priming_kept_full_gain=True, fade_waited_for_ready=True))
        stable_starts = cpu.starts.copy()
        play(4)
        require(cpu.starts == stable_starts, "repeated cue restarted playback")
        require(cpu.effects_remaining() == [cpu.death_remaining - 5 - visit] * 2,
                "effects did not continue after handoff")
        print(f"{standard}: switched {old_name} -> {expected} in {visit + 1} visits", flush=True)

    play(9)
    # Repeated requests preserve bounded prime progress at full old-track gain.
    def begin_fade():
        cpu.put("havoc_c64::REQUESTED_CUE", 1)
        cpu.put("havoc_c64::REQUESTED_STAGE", 0)
        previous = 0
        for _ in range(40):
            cpu.delay(0.06)
            service()
            produced = cpu.pending_frames()
            require(produced is not None and produced > previous and cpu.volume == [32, 32],
                    "pending request reset prime progress or muted active music")
            if cpu.state_value("fading"):
                require(produced == FRAMES, "fade started before priming")
                break
            previous = produced
        else:
            raise KernelError("pending track never reached fade phase")
        cpu.delay(0.2)
        service()
        require(cpu.active_name() == "hammond-rocker" and 0 < cpu.volume[0] < 32,
                "primed replacement did not begin fading")

    # Cancel both a partial prime and a running fade without restarting music.
    cpu.put("havoc_c64::REQUESTED_CUE", 1)
    starts = cpu.starts.copy()
    play(2)
    require(cpu.volume == [32, 32] and not cpu.state_value("fading"), "partial prime changed gain")
    cpu.put("havoc_c64::REQUESTED_CUE", 0)
    service()
    require(cpu.state_value("pending") == 255 and cpu.volume == [32, 32] and cpu.starts == starts,
            "partial prime cancellation restarted active music")
    cancel_history = len(cpu.volume_writes)
    begin_fade()
    cancelled_levels = [entry["value"] for entry in paired_gains(cpu.volume_writes[cancel_history:], "hammond-rocker")]
    require(cancelled_levels == sorted(cancelled_levels, reverse=True), "repeated pending cue restarted fade")
    cpu.put("havoc_c64::REQUESTED_CUE", 0)
    service()
    require(cpu.active_name() == "hammond-rocker" and cpu.starts == starts
            and cpu.volume == [32, 32] and cpu.state_value("pending") == 255 and not cpu.state_value("fading"),
            "active cue did not cancel pending and restore gain")
    # Replacing a pending track preserves the already elapsed fade. A forced
    # sample-counter rebase must not change the timer-based fade's progress.
    begin_fade()
    cpu.wrap_timer_soon()
    prior_gain, prior_tick = cpu.state_value("volume"), cpu.state_value("fade_tick")
    step = cpu.state_value("fade_step")
    require(0.999 <= step * 32 / cpu.bus_hz <= 1.001, "fade timer calibration")
    clock_address = cpu.rebase_sample_counters()
    cpu.put("havoc_c64::REQUESTED_CUE", 7)
    cpu.put("havoc_c64::REQUESTED_STAGE", 1)
    replace_history = len(cpu.volume_writes)
    service()
    consumed_ticks = (prior_tick - cpu.state_value("fade_tick")) & 0xffffffff
    require(consumed_ticks % step == 0
            and consumed_ticks // step == prior_gain - cpu.state_value("volume")
            and cpu.active_name() == "hammond-rocker" and cpu.starts == starts,
            "replacing pending cue restarted or skipped the fade")
    require(int.from_bytes(cpu.mem[clock_address:clock_address + 4], "little") < 0x01000000,
            "sample-counter rebase fixture did not execute")
    require((0xffffffff - (cpu.ticks - cpu.timer_origin)) & 0xffffffff > 0xff000000,
            "CIA down-counter wrap fixture did not execute")
    replacement_levels = [entry["value"] for entry in paired_gains(cpu.volume_writes[replace_history:], "hammond-rocker")]
    require(all(value <= prior_gain for value in replacement_levels), "replacement restored old music gain")
    play(5)
    require(cpu.volume == [0, 0] and cpu.state_value("fading") and cpu.pending_frames() < FRAMES
            and cpu.active_name() == "hammond-rocker" and cpu.starts == starts,
            "replaced fade did not wait muted for its new pending track")
    cpu.put("havoc_c64::REQUESTED_CUE", 0)
    service()
    require(cpu.volume == [32, 32] and cpu.starts == starts and cpu.state_value("pending") == 255
            and not cpu.state_value("fading"),
            "replacement cancellation did not restore active track")
    # Exercise every named recording, including changes with the SAME SID cue.
    for cue, stage, expected in ((1, 0, "lowland-approach"),
                                 (7, 1, "crystal-cave"),
                                 (2, 2, "iron-canyon"),
                                 (2, 3, "canyon-gate"),
                                 (8, 4, "ashen-wastes"),
                                 (8, 5, "foundry-belt")):
        select(cue, stage, expected)
        if stage == 3:
            starts = cpu.starts.copy()
            cpu.put("havoc_c64::REQUESTED_CUE", 10)  # Gorge Warden appears.
            play(3)
            require(cpu.active_name() == expected and cpu.starts == starts,
                    "Canyon Gate restarted at boss entrance")
    select(3, 6, "galactic-dash-mono")  # Unrecorded Fortress Run.
    starts = cpu.starts.copy()
    for stage in (18, 255):
        cpu.put("havoc_c64::REQUESTED_STAGE", stage)
        play(1)
        require(cpu.starts == starts and cpu.active_name() == "galactic-dash-mono",
                "invalid stage did not retain cue fallback")
    # Recorded stage indices cannot override menu, result or ascent cues.
    for cue in (6, 19):
        cpu.put("havoc_c64::REQUESTED_CUE", cue)
        cpu.put("havoc_c64::REQUESTED_STAGE", 0)
        play(1)
        require(cpu.starts == starts and cpu.active_name() == "galactic-dash-mono",
                "protected cue was overridden by stale gameplay stage")
    select(5, 0, "galactic-dash")
    select(0, 5, "hammond-rocker")
    select(3, 6, "galactic-dash-mono", cadence=0.02)  # Primed early; must wait for fade.
    require(cpu.prefix_clears >= 3, "mono prefixes not re-cleared across preparations")
    cpu.delay(1.25)
    service()
    require(cpu.get("DIGITAL_MINIMUM_LEAD", 2) > 2048, "recoverable gap crossed reader guard")
    play(1)
    cpu.delay(2.2)
    service(expect_fault=True)
    require(cpu.get("DIGITAL_FAULT") == 5 and cpu.control == cpu.volume == [0, 0], "underrun did not stop/mute")
    require(cpu.ring_banks == {0, 1} and cpu.ring_wraps > 0, "both slots and ring wraps")
    require(min(cpu.playback_checked) > 100000, "both hardware readers must be exercised")
    return dict(pcm_samples_checked=cpu.pcm_samples, starts=cpu.starts,
                sampler_channel_frames_checked=cpu.playback_checked, calibration_shots=cpu.calibrations,
                ring_wraps=cpu.ring_wraps, mono_prefix_clears=cpu.prefix_clears,
                fetches=cpu.fetches, stashes=cpu.stashes, max_dma_bytes=cpu.max_dma,
                minimum_lead_samples=cpu.get("DIGITAL_MINIMUM_LEAD", 2),
                main_stack_bytes_written=cpu.stack_top - cpu.stack_min, fault=cpu.get("DIGITAL_FAULT"),
                music_fades=fades,
                sampler_volume_writes=cpu.volume_writes, sampler_voice_starts=cpu.voice_starts,
                fade_edge_cases=dict(repeated_pending_preserves_progress=True, cancellation_restores_gain=True,
                                     replacement_preserves_progress=True, sample_rebase_during_fade=True,
                                     cia_wrap_during_fade=True,
                                     only_one_audible_source_slot=True, initial_full_gain=True),
                decode_budget=dict(maximum_running_audio_ms=max(call["decoded_audio_ms"] for call in cpu.calls
                    if call["entry"].endswith("service_audio") and call["active_track"] is not None),
                    limit_ms=250, scope="Active and pending decoded duration summed per service call; initial full-ring startup priming is exempt"),
                steady_service=steady_aggregates(cpu.calls, cpu.bus_hz), calls=cpu.calls)


def exercise_corruption(elf, symbols, image, standard, budget):
    """Immutable regions fail at boot; payload CRC fails during streamed prime
    only with the DIGITAL_STREAM_CRC debug hook, which production leaves off."""
    catalog = CATALOG["load_catalog"](validate=False)
    tracks = catalog["tracks"]
    track = next(entry for entry in tracks if entry["name"] == "hammond-rocker")
    results = {}
    directory = CATALOG["DIGITAL_OFFSET"]
    stage_map = directory + 16 + 16 * len(tracks) + len(catalog["cues"])
    cases = [("directory_version", directory + 4, 4),
             ("directory_stage_count", directory + 10, 4),
             ("directory_reserved", directory + 11, 4),
             ("directory_stage_id", stage_map + 3, 4),
             ("directory_checksum", directory + 14, 4)]
    cases += [(kind, track["offset"] + relative, fault)
              for kind, relative, fault in (("header", 20, 4), ("codebook", 32, 7),
                  ("crc_table", track["metadata"]["crc_offset"], 8),
                  ("stream", track["metadata"]["stream_offset"], 6))]
    mono = next(entry for entry in tracks if entry["name"] == "lowland-approach")
    cases.append(("mono_stream", mono["offset"] + mono["metadata"]["stream_offset"], 6))
    # The production decoder skips the per-byte stream CRC: a damaged payload
    # byte plays through (as wrong PCM) instead of muting the music.
    cases.append(("stream_unchecked", track["offset"] + track["metadata"]["stream_offset"], 0))
    for kind, offset, fault in cases:
        corrupted = bytearray(image)
        corrupted[offset] ^= 1
        cpu = Machine(elf, symbols, corrupted, standard)
        if kind.endswith("stream"):
            cpu.allow_corrupt_pcm = True
            cpu.put("DIGITAL_STREAM_CRC", 1)
            if kind == "mono_stream":
                cpu.put("havoc_c64::REQUESTED_CUE", 1)
                cpu.put("havoc_c64::REQUESTED_STAGE", 0)
            cpu.boot(budget)
            cpu.call("havoc_c64::service_audio", budget, expect_fault=True)
            require(cpu.control == cpu.volume == [0, 0], "CRC failure did not mute")
        elif kind == "stream_unchecked":
            cpu.allow_corrupt_pcm = True
            require(cpu.get("DIGITAL_STREAM_CRC") == 0, "stream CRC must default off")
            cpu.boot(budget)
            cpu.call("havoc_c64::service_audio", budget)
            require(cpu.get("MUSIC_BACKEND") == 1 and cpu.starts == [1, 1] and cpu.volume == [32, 32],
                    "unchecked stream corruption must keep playing")
        else:
            cpu.boot(budget, expect_fault=True)
        require(cpu.get("DIGITAL_FAULT") == fault, f"{kind} corruption fault classification")
        results[kind] = fault
        print(f"{standard}: corruption {kind} correctly reported fault {fault}", flush=True)
    return results


def exercise_emulator_sfx_only(elf, symbols, image, standard, budget):
    strict = Machine(elf, symbols, image, standard, sampler_present=False)
    strict.boot(budget, expect_fault=True)
    require(strict.get("DIGITAL_FAULT") == 1, "missing production sampler must report Mapping")
    cues = {}
    for cue, name in ((0, "title"), (5, "victory")):
        cpu = Machine(elf, symbols, image, standard, sampler_present=False)
        cpu.put("EMULATOR_SFX_ONLY", 1)
        cpu.put("havoc_c64::REQUESTED_CUE", cue)
        cpu.boot(budget)
        cpu.call("havoc_c64::service_audio", budget)
        require(cpu.get("MUSIC_BACKEND") == 3 and cpu.get("AUDIO_READY") == 1, f"SFX-only {name} init")
        cpu.queue_death_effect(budget)
        gated = set()
        for _ in range(50):
            cpu.call("havoc_c64::audio_tick", budget)
            cpu.call("havoc_c64::service_audio", budget)
            for sid in range(4):
                for voice in range(3):
                    base = 0xd400 + sid * 32 + voice * 7
                    if cpu.mem[base + 4] & 1 and any(cpu.mem[base:base + 2]):
                        gated.add((sid, voice))
        require(cpu.get("MUSIC_CUE") == cue and gated, f"SFX-only {name} never played")
        require(cpu.get("MUSIC_UNDERRUNS", 2) == cpu.get("DIGITAL_FAULT") == 0, "SFX-only fault")
        require(cpu.sampler_reads == cpu.sampler_writes == 0, "SFX-only touched absent sampler")
        cues[name] = dict(gated_sid_voices=len(gated), sampler_reads=cpu.sampler_reads, sampler_writes=cpu.sampler_writes)
    return dict(strict_production_fault=1, cues=cues, passed=True)


def exercise_fatal_shutdown(elf, symbols, image, standard):
    cpu = Machine(elf, symbols, image, standard)
    cpu.volume = [32, 32]
    cpu.control = [0x43, 0x43]
    for address in (1, 0xd031, 0xd01a, 0xd015, 0xdc0d, 0xdd0d):
        cpu.allowed[address] = True
    for address in (0xd418, 0xd438, 0xd458, 0xd478):
        cpu.mem[address] = 8
    cpu.pc = symbols["havoc_c64::fail"].address
    for _ in range(1000):
        cpu.step()
        if (cpu.volume == cpu.control == [0, 0]
                and all(cpu.mem[address] == 0 for address in (0xd418, 0xd438, 0xd458, 0xd478))):
            return dict(passed=True, muted_sids=4, muted_sampler_channels=2)
    raise KernelError("fatal shutdown did not mute all SID and sampler outputs")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", type=Path, default=ROOT / "dist/havoc.elf")
    parser.add_argument("--reu", type=Path, default=ROOT / "dist/havoc.reu")
    parser.add_argument("--standard", choices=("pal", "ntsc", "both"), default="both")
    parser.add_argument("--budget", type=int, default=100000000)
    parser.add_argument("--emulator-only", action="store_true")
    parser.add_argument("--kernel-only", action="store_true", help="only execute directed assembly decoder cases")
    parser.add_argument("--llvm-dwarfdump", type=Path,
                        default=Path(os.environ.get("LLVM_MOS", Path.home() / ".local/llvm-mos")) / "bin/llvm-dwarfdump")
    parser.add_argument("--report", type=Path, default=ROOT / "target/digital-kernel/report.json")
    args = parser.parse_args()
    elf, symbols = load_elf(args.elf)
    report = dict(model="NMOS6502 at64x bus; DMA1byte/bus tick; sampler6.25MHz/(divider+2)",
                  elf_sha256=hashlib.sha256(args.elf.read_bytes()).hexdigest(), standards={})
    if not args.emulator_only:
        report["assembly_kernel"] = exercise_decode_kernel(elf, symbols)
    if not args.kernel_only:
        preview = runpy.run_path(str(ROOT / "tools/sfx-previews.py"))
        elf.effects_layout = preview["read_effects_layout"](args.elf, args.llvm_dwarfdump)
        report["effects_layout"] = elf.effects_layout
        elf.digital_layout = read_digital_layout(args.elf, args.llvm_dwarfdump)
        report["digital_layout"] = elf.digital_layout
        require(symbols["havoc_c64::AUDIO"].size == elf.digital_layout["State"]["size"], "digital state ELF/DWARF size")
        image = args.reu.read_bytes()
        require(len(image) == symbols["__reu_size"].address, "REU/ELF size mismatch")
        report["reu_sha256"] = hashlib.sha256(image).hexdigest()
        for standard in ("pal", "ntsc") if args.standard == "both" else (args.standard,):
            result = {} if args.emulator_only else exercise(elf, symbols, image, standard, args.budget)
            result["emulator_sfx_only"] = exercise_emulator_sfx_only(elf, symbols, image, standard, args.budget)
            if not args.emulator_only:
                result["corruption_faults"] = exercise_corruption(elf, symbols, image, standard, args.budget)
            result["fatal_shutdown"] = exercise_fatal_shutdown(elf, symbols, image, standard)
            report["standards"][standard] = result
    report["passed"] = True
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(f"native DLT4 checks passed; {args.report}")


if __name__ == "__main__":
    main()
