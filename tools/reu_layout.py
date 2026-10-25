"""Checked v17 REU reservations shared by packaging and host inspection tools.

The linked symbols supply actual addresses; this fixed map independently checks
the linker contract without making the execution-window cap a storage size.
"""

PACKAGE_ID = 0x534d1100
PAGE_SHIFT = 11
MODEL_OFFSET = 0xb000
MUSIC_OFFSET = 0x80000
CODE_BASE = 0x14000
CODE_IMAGES = (
    ('boot', 'boot', 1), ('sim-lowlands', 'sim_lowlands', 2),
    ('geometry', 'geometry', 3), ('pixels', 'pixels', 4), ('audio', 'audio', 5),
    ('scenery-wildlife', 'scenery_wildlife', 7),
    ('sim-canyon', 'sim_canyon', 8), ('sim-fortress', 'sim_fortress', 9),
    ('sim-carrier', 'sim_carrier', 10), ('victory', 'victory', 12),
    ('sim-terminal', 'sim_terminal', 13), ('sim-warden', 'sim_warden', 18),
    ('sim-citadel', 'sim_citadel', 19), ('sim-flagship', 'sim_flagship', 20),
    ('scenery-transport', 'scenery_transport', 21),
    ('scenery-meteors', 'scenery_meteors', 22), ('sim-helpers', 'sim_helpers', 23),
    ('scenery-ascent', 'scenery_ascent', 24), ('scenery-asteroid', 'scenery_asteroid', 25),
    ('scenery-moon', 'scenery_moon', 26), ('scenery-graveyard', 'scenery_graveyard', 27),
    ('scenery-shipyard', 'scenery_shipyard', 28),
    ('scenery-carrier', 'scenery_carrier', 29), ('scenery-gate', 'scenery_gate', 30),
    ('shadow', 'shadow', 31), ('scenery-helpers', 'scenery_helpers', 33),
)
RESERVATIONS = dict.fromkeys((name for name, _, _ in CODE_IMAGES), 0x4800)
RESERVATIONS.update(boot=0x3000, audio=0x3800, victory=0x3800, shadow=0x3800)
RESERVATIONS.update({'sim-helpers': 0x2000, 'scenery-helpers': 0x1000})
OFFSETS = {}
CODE_END = CODE_BASE
for _name, _symbol, _logical_id in CODE_IMAGES:
    OFFSETS[_name] = CODE_END
    CODE_END += RESERVATIONS[_name]
assert CODE_END == 0x7e800


def image_size_symbol(name):
    if name in ('sim-helpers', 'scenery-helpers'):
        profile = name.replace('-helpers', '_aux')
    elif name.startswith('sim-'):
        profile = 'sim'
    elif name.startswith('scenery-'):
        profile = 'scenery'
    else:
        profile = name
    return f'__phase_{profile}_size'


def checked_layout(symbols):
    """Reject invalid linked reservations or payloads before writing an image."""
    size = symbols['__reu_size']
    assert size == 0x1000000, 'incorrect 16 MiB REU image size'
    assert symbols['__reu_code_slot_size'] == 0x4200, 'incorrect REU code slot size'
    assert symbols['__reu_digital_music'] == MUSIC_OFFSET, 'incorrect digital music address'
    assert symbols['__reu_digital_victory'] == MUSIC_OFFSET, 'incorrect digital victory alias'
    assert symbols['__reu_digital_ring'] == 0xfe0000, 'incorrect digital ring address'
    assert symbols['__reu_digital_ring_size'] == 0x20000, 'incorrect digital ring size'
    assert symbols['__reu_models'] == MODEL_OFFSET, 'incorrect REU model bank address'
    assert symbols['__reu_models_end'] == CODE_BASE, 'incorrect REU model bank end'
    assert (symbols['__reu_probe_start'], symbols['__reu_probe_end']) == (0xc00, 0x1000), 'incorrect REU probe reservation'
    allocations = [(0, 0x180, 'directories'), (0xc00, 0x1000, 'hardware probe'),
                   (0x1000, 0x1c00, 'common assets'), (0x2000, 0xb000, 'encounters'),
                   (MODEL_OFFSET, CODE_BASE, 'models'),
                   (MUSIC_OFFSET, 0xfe0000, 'digital soundtrack'),
                   (0xfe0000, size, 'mutable PCM ring')]
    offsets = {}
    for name, symbol, _ in CODE_IMAGES:
        address, end = symbols[f'__reu_{symbol}'], symbols[f'__reu_{symbol}_end']
        assert 0 <= address < end <= MUSIC_OFFSET, f'{name} REU slot exceeds image code region'
        assert address & 0x7ff == 0 and end & 0x7ff == 0, f'{name} REU slot must be 2 KiB aligned'
        assert end - address == RESERVATIONS[name], f'incorrect {name} REU reservation size'
        length = symbols[image_size_symbol(name)]
        assert 0 < length <= 0x4200, f'{name} exceeds execution window'
        assert length <= end - address, f'{name} exceeds REU reservation'
        for start, stop, owner in allocations:
            assert end <= start or address >= stop, f'{name} REU slot overlaps {owner}'
        offsets[name] = address
        allocations.append((address, end, name))
    for name, address in offsets.items():
        assert address == OFFSETS[name], f'incorrect {name} REU slot address'
    return offsets, allocations
