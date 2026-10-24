#!/usr/bin/env python3
"""Execute linked DLT4 decoding, BOOT, and AUDIO against independent models.

This is modeled NMOS6502 execution at 64x PAL/NTSC bus speed, with DMA charged
one bus tick/byte. The sampler clock is 6.25MHz/(divider+2); the extra two ticks
are synthetic calibration stress, not a physical hardware measurement.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import runpy
import struct

ROOT = Path(__file__).resolve().parents[1]
KERNEL = runpy.run_path(str(ROOT / "tools/test-music-kernel.py"))
CATALOG = runpy.run_path(str(ROOT / "tools/digital_format.py"))
Cpu, KernelError, load_elf = (KERNEL[name] for name in ("Cpu", "KernelError", "load_elf"))
SENTINEL = 0xffff
FRAMES = 32768
DELAY = 160
DECODE_BUDGET_UNITS = 20000  # 80,000 units/second: 4 per stereo frame, 5 per mono frame.
MAX_HANDOFF_VISITS = 128


def require(condition, message):
    if not condition:
        raise KernelError(message)


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
        for name in (".data", ".bss", ".noinit", ".audio_state", ".game_state", ".music_page"):
            section = self.sections.get(name)
            if section:
                writable.append((section.address, section.address + section.size))
        for name in ("havoc_c64::AUDIO", "havoc_c64::EFFECTS", "havoc_c64::GAME", "havoc_c64::ERROR_DIGITAL"):
            symbol = symbols[name]
            writable.append((symbol.address, symbol.address + symbol.size))
        writable += [(symbols["__stack_bottom"].address, symbols["__stack"].address),
                     (symbols["__irq_stack_bottom"].address, symbols["__irq_stack"].address),
                     (symbols["__music_page_start"].address, symbols["__music_page_end"].address),
                     (symbols["music_kernel_tick"].address, symbols["music_kernel_tick"].address + symbols["__music_kernel_size"].address)]
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
        self.low_bytes = [0xa5, 0x5a]  # 8-bit reads must not inherit these.
        self.calibrations = self.wide_clears = 0
        self.playback, self.playback_checked = [None, None], [0, 0]
        self.fetches = self.stashes = self.max_dma = self.ring_wraps = self.prefix_clears = 0
        self.ring_banks = set()
        self.catalog = CATALOG["load_catalog"](validate=False)
        self.references = self.catalog["tracks"]
        self.decoder_tracks = {}
        self.decoder_slots = {}
        self.staging_decoder = None
        self.rings = [None, None]
        self.pcm_samples = {entry["name"]: 0 for entry in self.references}
        self.decoded_samples = self.pcm_samples.copy()
        self.decoded_units = 0
        self.calls = []
        self.reported_profiles = set()
        self.first_kernel_call = None
        self.allow_corrupt_pcm = False
        self.put("MUSIC_BACKEND", 2)
        self.put("havoc_c64::REQUESTED_CUE", 0)
        self.put("havoc_c64::MUSIC_VALIDATED", 0)
        self.put("havoc_c64::CLOCK_KHZ", 985 if standard == "pal" else 1023, 2)
        self.put("havoc_c64::MUSIC_DIRECTORY_HASH", CATALOG["checksum"](image[0xc0000:0xc0100]), 2)
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

    def prime(self, budget):
        """Drive the real per-frame startup without bypassing its decoding cap."""
        require(self.active_name() is None and self.starts == [0, 0], "prime requires stopped startup")
        name = self.requested_track()
        entry = next(entry for entry in self.references if entry["name"] == name)
        cost = 4 if entry["metadata"]["channels"] == 2 else 5
        per_visit = DECODE_BUDGET_UNITS // cost
        visits = (FRAMES + per_visit - 1) // per_visit
        for visit in range(visits):
            if visit:
                self.delay(0.02)
            self.call("havoc_c64::service_audio", budget)
            require(self.get("MUSIC_BACKEND") == 1, "priming must retain DAC ownership")
            require(self.pcm_samples[name] == min(FRAMES, (visit + 1) * per_visit),
                    "initial prime did not use its bounded per-visit allowance")
            if visit + 1 < visits:
                require(self.starts == [0, 0] and self.control == self.volume == [0, 0],
                        "sampler started before the initial ring was complete")
        require(self.starts == [1, 1] and self.active_name() == name, "completed prime did not start once")
        return visits

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
            self.volume[(address - 0xdf21) // 0x20] = value
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

    def start_voice(self, channel, address, control):
        # Adjacent stores alone are insufficient: a real IRQ between them
        # permanently separates the two sampler clocks.
        require(self.p & KERNEL["I"], "sampler starts must mask IRQs")
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

    def be16(self, address):
        return int.from_bytes(self.mem[address:address + 2], "big")

    def be24(self, address):
        return int.from_bytes(self.mem[address:address + 3], "big")

    def active_name(self):
        voice = self.playback[0]
        return None if voice is None else self.rings[voice["slot"]]["track"]["name"]

    def queue_death_effect(self, budget):
        # Public repr(C), 36-byte EffectsEngine image: events follow the two
        # five-byte voices; event_count follows the fourteen shadow bytes.
        effects = self.symbols["havoc_c64::EFFECTS"]
        require(effects.size == 36, "effects fixture ABI")
        self.mem[effects.address + 10:effects.address + 12] = bytes((7, 0))
        self.mem[effects.address + 33] = 1
        self.call("havoc_c64::audio_tick", budget)
        require(self.mem[effects.address + 3] == self.mem[effects.address + 8] == 40,
                "digital playback did not start the SID death effect")
        require(self.mem[0xd412] & 1 and self.mem[0xd432] & 1, "effect SID gates")
        return effects.address

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
                    stream = entry["offset"] + entry["metadata"]["stream_offset"]
                    if stream <= reu < stream + entry["metadata"]["stream_bytes"]:
                        audio = self.symbols["havoc_c64::AUDIO"]
                        require(audio.address <= ram and ram + count <= audio.address + audio.size,
                                "compressed cache must belong to a decoder")
                        # Decoder's repr(C) cache immediately follows its 40-byte
                        # Kernel. Keep ownership per decoder: a short cached
                        # tail may follow a fetch for the other ring slot.
                        self.decoder_tracks[ram - 40] = entry
                        break
        elif command == 0x90:
            require(self.ring <= reu and reu + count <= self.ring + self.ring_size, "write outside mutable PCM arena")
            data = self.mem[ram:ram + count]
            slot, offset = divmod(reu - self.ring, 65536)
            self.ring_banks.add(slot)
            if self.phase == "boot":
                require(not any(data) and self.control == [0, 0], "boot ring clear must be silent")
            elif self.staging_decoder is None and offset == 32768 - DELAY and count == DELAY:
                require(not any(data), "mono delay prefix was not cleared on preparation")
                require(all(v is None or v["slot"] != slot for v in self.playback), "prefix changed under active reader")
                self.prefix_clears += 1
            else:
                decoder = self.staging_decoder
                require(decoder in self.decoder_tracks, "PCM publication without decoder ownership")
                entry = self.decoder_tracks[decoder]
                previous_slot = self.decoder_slots.setdefault(decoder, slot)
                require(previous_slot == slot, "decoder changed PCM ring slot")
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
                self.staging_decoder = None
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
        decoded_before, units_before = self.decoded_samples.copy(), self.decoded_units
        active = self.active_name()
        starts = self.starts.copy()
        divider = self.be16(0xdf2e) if active is not None else None
        requested = self.requested_track()
        steady = name.endswith("service_audio") and active is not None and active == requested
        failure = self.symbols["havoc_c64::fail"].address
        decoder = self.symbols["digital_decode_run"].address
        for steps in range(budget):
            if self.pc in (SENTINEL, failure):
                break
            if self.phase == "audio" and self.pc == decoder:
                require(not self.p & KERNEL["I"], "PCM preparation must remain interruptible")
                pointer = int.from_bytes(self.mem[4:6], "little")
                entry = self.decoder_tracks.get(pointer)
                require(entry is not None, "decode kernel has no compressed-cache owner")
                require(self.staging_decoder in (None, pointer), "mixed decoder output in one PCM batch")
                self.staging_decoder = pointer
                channels = self.mem[pointer + 39]
                require(channels == entry["metadata"]["channels"] and 1 <= self.a <= 128,
                        "decode kernel profile/frame count")
                self.decoded_samples[entry["name"]] += self.a
                self.decoded_units += self.a * (4 if channels == 2 else 5)
                if name.endswith("service_audio"):
                    require(self.decoded_units - units_before <= DECODE_BUDGET_UNITS,
                            "audio service decoded more than 250 ms across both slots")
                if self.first_kernel_call is None:
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
            if name.endswith("service_audio"):
                require(not self.p & KERNEL["I"], "audio service must restore IRQs")
                require(self.staging_decoder is None, "audio service left decoded PCM unpublished")
        result = dict(entry=name, instructions=steps, cpu_cycles=self.cycles - start,
                      bus_ticks=self.ticks - ticks, pcm_samples=sum(self.pcm_samples.values()) - samples,
                      decoded_frames={track: count - decoded_before[track]
                                      for track, count in self.decoded_samples.items()
                                      if count != decoded_before[track]},
                      decoded_budget_units=self.decoded_units - units_before,
                      active_track=active,
                      sampler_divider=divider,
                      modeled_sampler_rate_hz=None if divider is None else 6250000 / (divider + 2),
                      steady=steady and not faulted and self.active_name() == active and self.starts == starts,
                      fault=faulted)
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
    stereo_prime_visits = cpu.prime(budget)
    require(stereo_prime_visits == 7, "stereo startup must take seven bounded visits")
    require(cpu.active_name() == "hammond-rocker" and cpu.wide_clears > 0, "stereo title and cleared sampler")
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

    def select(cue, stage, expected):
        old_name, starts = cpu.active_name(), cpu.starts.copy()
        effects = cpu.queue_death_effect(budget)
        remaining = 40
        cpu.put("havoc_c64::REQUESTED_CUE", cue)
        cpu.put("havoc_c64::REQUESTED_STAGE", stage)
        require(cpu.requested_track() == expected, "fixture scene-to-track mapping")
        # The active stream spends part of the same 250 ms allowance. At this
        # 180 ms cadence, preparation takes substantially more than eight visits.
        for visit in range(MAX_HANDOFF_VISITS):
            # Keep both effects live through the swap and the four follow-up
            # ticks. Refresh only after the previous visit checked continuity.
            if remaining <= 5:
                cpu.queue_death_effect(budget)
                remaining = 40
            cpu.delay(0.18)
            service()
            cpu.call("havoc_c64::audio_tick", budget)
            remaining -= 1
            require(cpu.mem[effects + 3] == remaining and cpu.mem[effects + 8] == remaining,
                    "cue preparation or handoff reset/stalled SID effects")
            if cpu.active_name() == expected:
                break
            require(cpu.active_name() == old_name, "pending preparation interrupted active music")
        else:
            raise KernelError(f"pending track never completed: {expected}")
        entry = next(entry for entry in cpu.references if entry["name"] == expected)
        cost = 4 if entry["metadata"]["channels"] == 2 else 5
        minimum_visits = (FRAMES * cost + DECODE_BUDGET_UNITS - 1) // DECODE_BUDGET_UNITS
        require(visit + 1 >= minimum_visits and cpu.starts == [value + 1 for value in starts],
                "bounded single handoff")
        require(cpu.calibrations == calibration_count, "transition recalibrated")
        stable_starts = cpu.starts.copy()
        require(remaining > 4, "handoff must preserve live effects through follow-up ticks")
        play(4)
        remaining -= 4
        require(cpu.starts == stable_starts, "repeated cue restarted playback")
        require(cpu.mem[effects + 3] == remaining and cpu.mem[effects + 8] == remaining,
                "effects did not continue after handoff")
        print(f"{standard}: switched {old_name} -> {expected} in {visit + 1} visits", flush=True)

    play(9)
    # Cancel a partially prepared replacement by requesting the active track.
    cpu.put("havoc_c64::REQUESTED_CUE", 1)
    play(2)
    starts = cpu.starts.copy()
    cpu.put("havoc_c64::REQUESTED_CUE", 0)
    play(2)
    require(cpu.active_name() == "hammond-rocker" and cpu.starts == starts, "active cue did not cancel pending")
    # Exercise every named recording, including changes with the SAME SID cue.
    for cue, stage, expected in ((1, 0, "lowland-approach"),
                                 (7, 1, "crystal-cave"),
                                 (2, 2, "iron-canyon"),
                                 (2, 3, "canyon-gate"),
                                 (8, 4, "ashen-wastes"),
                                 (8, 5, "foundry-belt"),
                                 (3, 6, "fortress-run")):
        select(cue, stage, expected)
        if stage == 3:
            starts = cpu.starts.copy()
            cpu.put("havoc_c64::REQUESTED_CUE", 10)  # Gorge Warden appears.
            play(3)
            require(cpu.active_name() == expected and cpu.starts == starts,
                    "Canyon Gate restarted at boss entrance")
    # Citadel Core shares the Fortress cue but still uses the unrecorded-stage fallback.
    select(3, 7, "galactic-dash-mono")
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
    select(3, 7, "galactic-dash-mono")  # Reuses a former stereo slot and prefix.
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
    mono = Machine(elf, symbols, image, standard)
    mono.put("havoc_c64::REQUESTED_CUE", 1)
    mono.put("havoc_c64::REQUESTED_STAGE", 0)
    mono.boot(budget)
    mono_prime_visits = mono.prime(budget)
    require(mono_prime_visits == 9, "mono startup must take nine bounded visits")
    return dict(pcm_samples_checked=cpu.pcm_samples, starts=cpu.starts,
                initial_prime_visits=dict(stereo=stereo_prime_visits, mono=mono_prime_visits),
                mono_initial_prime_calls=[call for call in mono.calls if call["entry"].endswith("service_audio")],
                sampler_channel_frames_checked=cpu.playback_checked, calibration_shots=cpu.calibrations,
                ring_wraps=cpu.ring_wraps, mono_prefix_clears=cpu.prefix_clears,
                fetches=cpu.fetches, stashes=cpu.stashes, max_dma_bytes=cpu.max_dma,
                minimum_lead_samples=cpu.get("DIGITAL_MINIMUM_LEAD", 2),
                main_stack_bytes_written=cpu.stack_top - cpu.stack_min, fault=cpu.get("DIGITAL_FAULT"),
                steady_service=steady_aggregates(cpu.calls, cpu.bus_hz), calls=cpu.calls)


def exercise_corruption(elf, symbols, image, standard, budget):
    """Immutable regions fail at boot; payload CRC fails during streamed prime."""
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
    for kind, offset, fault in cases:
        corrupted = bytearray(image)
        corrupted[offset] ^= 1
        cpu = Machine(elf, symbols, corrupted, standard)
        if kind.endswith("stream"):
            cpu.allow_corrupt_pcm = True
            if kind == "mono_stream":
                cpu.put("havoc_c64::REQUESTED_CUE", 1)
                cpu.put("havoc_c64::REQUESTED_STAGE", 0)
            cpu.boot(budget)
            cpu.call("havoc_c64::service_audio", budget, expect_fault=True)
            require(cpu.control == cpu.volume == [0, 0] and cpu.starts == [0, 0],
                    "CRC failure during initial priming did not remain stopped")
        else:
            cpu.boot(budget, expect_fault=True)
        require(cpu.get("DIGITAL_FAULT") == fault, f"{kind} corruption fault classification")
        results[kind] = fault
        print(f"{standard}: corruption {kind} correctly reported fault {fault}", flush=True)
    return results


def exercise_emulator_sid_only(elf, symbols, image, standard, budget):
    strict = Machine(elf, symbols, image, standard, sampler_present=False)
    strict.boot(budget, expect_fault=True)
    require(strict.get("DIGITAL_FAULT") == 1, "missing production sampler must report Mapping")
    cues = {}
    for cue, name in ((0, "title"), (5, "victory")):
        cpu = Machine(elf, symbols, image, standard, sampler_present=False)
        cpu.put("EMULATOR_SID_ONLY", 1)
        cpu.put("havoc_c64::REQUESTED_CUE", cue)
        cpu.boot(budget)
        cpu.call("havoc_c64::service_audio", budget)
        require(cpu.get("MUSIC_BACKEND") == 0 and cpu.get("MUSIC_VALIDATED") == 1, f"SID-only {name} init")
        gated = set()
        for _ in range(50):
            cpu.call("havoc_c64::audio_tick", budget)
            cpu.call("havoc_c64::service_audio", budget)
            for sid in range(4):
                for voice in range(3):
                    base = 0xd400 + sid * 32 + voice * 7
                    if cpu.mem[base + 4] & 1 and any(cpu.mem[base:base + 2]):
                        gated.add((sid, voice))
        require(cpu.get("MUSIC_CUE") == cue and gated, f"SID-only {name} never played")
        require(cpu.get("MUSIC_UNDERRUNS", 2) == cpu.get("DIGITAL_FAULT") == 0, "SID-only fault")
        require(cpu.sampler_reads == cpu.sampler_writes == 0, "SID-only touched absent sampler")
        cues[name] = dict(gated_sid_voices=len(gated), sampler_reads=cpu.sampler_reads, sampler_writes=cpu.sampler_writes)
    return dict(strict_production_fault=1, cues=cues, passed=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", type=Path, default=ROOT / "dist/havoc.elf")
    parser.add_argument("--reu", type=Path, default=ROOT / "dist/havoc.reu")
    parser.add_argument("--standard", choices=("pal", "ntsc", "both"), default="both")
    parser.add_argument("--budget", type=int, default=100000000)
    parser.add_argument("--emulator-only", action="store_true")
    parser.add_argument("--kernel-only", action="store_true", help="only execute directed assembly decoder cases")
    parser.add_argument("--report", type=Path, default=ROOT / "target/digital-kernel/report.json")
    args = parser.parse_args()
    elf, symbols = load_elf(args.elf)
    report = dict(model="NMOS6502 at64x bus; DMA1byte/bus tick; sampler6.25MHz/(divider+2)",
                  elf_sha256=hashlib.sha256(args.elf.read_bytes()).hexdigest(), standards={})
    if not args.emulator_only:
        report["assembly_kernel"] = exercise_decode_kernel(elf, symbols)
    if not args.kernel_only:
        image = args.reu.read_bytes()
        require(len(image) == symbols["__reu_size"].address, "REU/ELF size mismatch")
        report["reu_sha256"] = hashlib.sha256(image).hexdigest()
        for standard in ("pal", "ntsc") if args.standard == "both" else (args.standard,):
            result = {} if args.emulator_only else exercise(elf, symbols, image, standard, args.budget)
            result["emulator_sid_only"] = exercise_emulator_sid_only(elf, symbols, image, standard, args.budget)
            if not args.emulator_only:
                result["corruption_faults"] = exercise_corruption(elf, symbols, image, standard, args.budget)
            report["standards"][standard] = result
    report["passed"] = True
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(f"native DLT4 checks passed; {args.report}")


if __name__ == "__main__":
    main()
