; Resident quad-SID effects engine: the native body of
; havoc::audio::effects::EffectsEngine::tick with its start/stop paths.
; tools/test-sfx-kernel.py compares the linked player against complete host
; state/register/write fixtures exported by `cargo run --release --
; sfx-fixtures`, so every SID write below keeps the Rust reference's order
; and its changed-register-only cache. The queue helper stays compiled Rust:
; an external callee would make every caller of Game::sound spill around the
; call under interprocedural register allocation.
;
; void fast_effects_tick(void)
;   C ABI: no arguments or result. Operates on the EFFECTS engine at its
;   linker address and on the four chips at $D400/$D420/$D440/$D460 (I/O
;   must be mapped). Only caller-saved rc2..19 and A/X/Y change; rc20..27
;   are used as scratch but saved and restored on the hardware stack.
;   Called from irq_body; no software stack frame and no compiler static
;   frame (tools/check-irq-frames.py).
;
; Layouts are fixed by const assertions beside the Rust declarations
; (src/audio/effects.rs, src/audio/patches.rs).
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7
.zeropage __rc8
.zeropage __rc9
.zeropage __rc10
.zeropage __rc11
.zeropage __rc12
.zeropage __rc13
.zeropage __rc14
.zeropage __rc15
.zeropage __rc16
.zeropage __rc17
.zeropage __rc18
.zeropage __rc19
.zeropage __rc20
.zeropage __rc21
.zeropage __rc22
.zeropage __rc23
.zeropage __rc24
.zeropage __rc25
.zeropage __rc26
.zeropage __rc27

; EffectsEngine (200 bytes): instances[4] @0, registers[128] @52,
; events[8] @180, event_count @196, standard @197, enabled @198, rotation @199.
.set EFX_REGS, 52
.set EFX_EVENTS, 180
.set EFX_COUNT, 196
.set EFX_STANDARD, 197
.set EFX_ENABLED, 198
.set EFX_ROTATION, 199
; Instance (13 bytes): mask, effect, age, remaining, frequency[3] u16,
; cutoff u16, program.
.set I_MASK, 0
.set I_EFFECT, 1
.set I_AGE, 2
.set I_REMAIN, 3
.set I_FREQ, 4
.set I_CUTOFF, 10
.set I_PROGRAM, 12
.set INST_SIZE, 13
.set INST_END, 4 * INST_SIZE
; Patch (25 bytes) and Program (11 bytes) fields; see src/audio/patches.rs.
.set P_NOTES, 0
.set P_WAVES, 3
.set P_AD, 6
.set P_SR, 9
.set P_PULSE, 12
.set P_CUTOFF, 15
.set P_ATTACK, 18
.set P_RELEASE, 19
.set P_DURATION, 20
.set P_FILTER, 21
.set P_RESONANCE, 22
.set P_LATE_RES, 23
.set P_PROGRAMS, 24
.set PATCH_SIZE, 25
.set G_SLIDE, 0
.set G_PWM_RATE, 3
.set G_PWM_DEPTH, 4
.set G_CHANGE, 5
.set G_LATE_WAVES, 6
.set G_RETRIGGER, 9
.set G_ACCENT, 10
.set PROGRAM_SIZE, 11
.set ACCENT_ARPEGGIO, 1
.set ACCENT_OCTAVE, 2
.set ACCENT_VIBRATO, 3
.set ACCENT_PWM_ALL, 4
.set VOLUME, 8
.set PUNCH_VOLUME, 15
.set EFFECT_BOMB, 6
.set EFFECT_BOSS_EXPLOSION, 15
.set EFFECT_PLAYER_DEATH, 16

; ---- tick scratch (per-chip loop)
.set SID, __rc2                 ; chip 0..3
.set SIDBASE, __rc3             ; chip << 5: register cache / hardware offset
.set PATCH, __rc4               ; /5 pointer to the instance's patch
.set PROG, __rc6                ; program * 11 (byte index into sfx_programs)
.set ACCENT, __rc7              ; the program's accent byte
.set AGE, __rc8
.set PROGNUM, __rc9
.set CHANGE, __rc10
.set RELEASE, __rc11
.set VOICE, __rc12              ; voice 0..2
.set VREG, __rc13               ; SIDBASE | voice * 7
.set FREQ, __rc14               ; /15 u16 work
.set T, __rc16                  ; /17 u16 work
.set WAVE, __rc18
.set SLIDE, __rc19
; ---- stop scratch (also live during start)
.set STOP_MASK, __rc20
.set STOP_X, __rc21
.set STOP_BASE, __rc22
.set STOP_I, __rc23
; ---- event admission / start scratch
.set ADMITTED, __rc24
.set EV_PRIO, __rc25
.set NCAND, __rc26
.set INST, __rc27               ; instance offset (chip * 13)
.set BESTIDX, __rc25            ; event selection only
.set I, __rc26
.set EV_EFFECT, __rc8
.set EV_PAN, __rc9
.set LANE, __rc10
.set RESTART, __rc11
; BEST survives into the allocation loop, which reuses VOICE..WAVE (rc12..18).
.set BEST, __rc19
.set VICTIMS, __rc13
.set BESTCOST, __rc14
.set BESTAGE, __rc15
.set CAND, __rc16
.set MASK, __rc17
.set FULL, __rc18
.set HIGHEST, __rc7
.set OCCUPIED, __rc2
.set YOUNGEST, __rc3
.set BITS, __rc4
.set PRIO, __rc5
.set COST, __rc6

.section .text.fast_effects_tick,"ax",@progbits
.globl fast_effects_tick
.type fast_effects_tick,@function
fast_effects_tick:
    lda EFFECTS+EFX_ENABLED
    bne .Lt_enabled
    rts
.Lt_enabled:
    ldx #0
.Lt_save:
    lda __rc20,x
    pha
    inx
    cpx #8
    bne .Lt_save
    lda #0
    sta SID
    sta SIDBASE
    tax
.Lt_sid:
    stx INST
    lda EFFECTS+I_MASK,x
    bne .Lt_live
    jmp .Lt_next_sid
.Lt_live:
    lda EFFECTS+I_REMAIN,x
    cmp #2
    bcs .Lt_alive
    lda EFFECTS+I_MASK,x
    jsr sfx_stop
    jmp .Lt_next_sid
.Lt_alive:
    dec EFFECTS+I_REMAIN,x
    inc EFFECTS+I_AGE,x
    lda EFFECTS+I_AGE,x
    sta AGE
    ldy EFFECTS+I_EFFECT,x
    jsr .Lpatch_pointer
    lda EFFECTS+I_PROGRAM,x
    sta PROGNUM
    tay
    lda .Lprog_off,y
    sta PROG
    tay
    lda sfx_programs+G_CHANGE,y
    sta CHANGE
    lda sfx_programs+G_ACCENT,y
    sta ACCENT
    ldy #P_RELEASE
    lda (PATCH),y
    sta RELEASE
    ; ---- filter target: cutoff[1] during the attack, cutoff[2] after,
    ; each an eight-bit value times eight
    ldy #P_ATTACK
    lda AGE
    cmp (PATCH),y
    bcs .Lt_cut_late
    ldy #P_CUTOFF+1
    bne .Lt_cut_load
.Lt_cut_late:
    ldy #P_CUTOFF+2
.Lt_cut_load:
    lda (PATCH),y
    jsr .Lcutoff_target
    ; the right chip of a stereo pair sits 23 steps higher, clamped to 2047
    lda SID
    lsr
    bcc .Lt_target_ok
    lda T
    clc
    adc #23
    sta T
    bcc .Lt_target_clamp
    inc T+1
.Lt_target_clamp:
    lda T+1
    cmp #8
    bcc .Lt_target_ok
    lda #$ff
    sta T
    lda #7
    sta T+1
.Lt_target_ok:
    lda EFFECTS+I_CUTOFF,x
    cmp T
    bne .Lt_move
    lda EFFECTS+I_CUTOFF+1,x
    cmp T+1
    beq .Lt_cut_write
.Lt_move:
    ; FREQ = |target - cutoff|; SLIDE = 1 when the cutoff must fall
    sec
    lda T
    sbc EFFECTS+I_CUTOFF,x
    sta FREQ
    lda T+1
    sbc EFFECTS+I_CUTOFF+1,x
    sta FREQ+1
    lda #0
    sta SLIDE
    bcs .Lt_step
    sec
    lda #0
    sbc FREQ
    sta FREQ
    lda #0
    sbc FREQ+1
    sta FREQ+1
    inc SLIDE
.Lt_step:
    ; step = distance >> rate, at least one
    ldy #P_RESONANCE
    lda (PATCH),y
    and #7
    tay
    beq .Lt_step_min
.Lt_shift:
    lsr FREQ+1
    ror FREQ
    dey
    bne .Lt_shift
.Lt_step_min:
    lda FREQ
    ora FREQ+1
    bne .Lt_apply
    inc FREQ
.Lt_apply:
    lda SLIDE
    bne .Lt_down
    clc
    lda EFFECTS+I_CUTOFF,x
    adc FREQ
    sta EFFECTS+I_CUTOFF,x
    lda EFFECTS+I_CUTOFF+1,x
    adc FREQ+1
    sta EFFECTS+I_CUTOFF+1,x
    jmp .Lt_cut_write
.Lt_down:
    sec
    lda EFFECTS+I_CUTOFF,x
    sbc FREQ
    sta EFFECTS+I_CUTOFF,x
    lda EFFECTS+I_CUTOFF+1,x
    sbc FREQ+1
    sta EFFECTS+I_CUTOFF+1,x
.Lt_cut_write:
    lda EFFECTS+I_CUTOFF,x
    sta FREQ
    lda EFFECTS+I_CUTOFF+1,x
    sta FREQ+1
    jsr .Lcutoff_write
    ; ---- timbre transition resonance
    lda AGE
    cmp CHANGE
    bne .Lt_punch
    ldy #P_LATE_RES
    lda (PATCH),y
    and #$f0
    beq .Lt_punch
    sta T
    ldy #P_FILTER
    lda (PATCH),y
    and #7
    ora T
    sta T
    lda SIDBASE
    ora #23
    tay
    lda T
    jsr sfx_write
.Lt_punch:
    ; ---- volume punch: full chip volume for the authored ticks, then one
    ; step down per tick to the mix level
    ldy #P_LATE_RES
    lda (PATCH),y
    and #15
    beq .Lt_voices
    sta T
    lda AGE
    cmp T
    bcc .Lt_vol_full
    sec
    sbc T
    cmp #PUNCH_VOLUME-VOLUME
    bcs .Lt_vol_floor
    eor #$ff
    clc
    adc #PUNCH_VOLUME+1
    bne .Lt_vol_set
.Lt_vol_floor:
    lda #VOLUME
    bne .Lt_vol_set
.Lt_vol_full:
    lda #PUNCH_VOLUME
.Lt_vol_set:
    sta T
    ldy #P_FILTER
    lda (PATCH),y
    and #$f0
    ora T
    sta T
    lda SIDBASE
    ora #24
    tay
    lda T
    jsr sfx_write
.Lt_voices:
    lda #0
    sta VOICE
    lda SIDBASE
    sta VREG
.Lt_voice:
    ; ---- waveform, with the staged late waveform once the program changes
    lda VOICE
    clc
    adc #P_WAVES
    tay
    lda (PATCH),y
    sta WAVE
    lda AGE
    cmp CHANGE
    bcc .Lt_wave_done
    lda PROG
    clc
    adc VOICE
    tay
    lda sfx_programs+G_LATE_WAVES,y
    beq .Lt_wave_done
    sta WAVE
.Lt_wave_done:
    ; ---- pitch slide: freq -= freq >> -slide, or freq += freq >> slide
    lda PROG
    clc
    adc VOICE
    tay
    lda sfx_programs+G_SLIDE,y
    sta SLIDE
    lda INST
    clc
    adc VOICE
    adc VOICE
    tax
    lda EFFECTS+I_FREQ,x
    sta FREQ
    lda EFFECTS+I_FREQ+1,x
    sta FREQ+1
    lda SLIDE
    beq .Lt_slide_done
    bmi .Lt_slide_down
    tay
    jsr .Lshifted
    jsr .Lfreq_add_saturating
    jmp .Lt_slide_done
.Lt_slide_down:
    eor #$ff
    clc
    adc #1
    tay
    jsr .Lshifted
    jsr .Lfreq_subtract
.Lt_slide_done:
    ; the arpeggio accent steps up by an eighth every fourth tick
    lda ACCENT
    and #3
    cmp #ACCENT_ARPEGGIO
    bne .Lt_no_arp
    lda AGE
    and #3
    bne .Lt_no_arp
    ldy #3
    jsr .Lshifted
    jsr .Lfreq_add_saturating
.Lt_no_arp:
    lda FREQ
    sta EFFECTS+I_FREQ,x
    lda FREQ+1
    sta EFFECTS+I_FREQ+1,x
    ; ---- rendered pitch: octave toggle or vibrato accents
    lda ACCENT
    and #3
    cmp #ACCENT_OCTAVE
    bne .Lt_not_octave
    lda AGE
    and #4
    beq .Lt_rendered
    lsr FREQ+1
    ror FREQ
    jmp .Lt_rendered
.Lt_not_octave:
    cmp #ACCENT_VIBRATO
    bne .Lt_rendered
    ldy #5
    jsr .Lshifted
    lda AGE
    and #2
    beq .Lt_vibrato_down
    jsr .Lfreq_add_saturating
    jmp .Lt_rendered
.Lt_vibrato_down:
    jsr .Lfreq_subtract
.Lt_rendered:
    lda SLIDE
    bne .Lt_write_freq
    lda ACCENT
    and #3
    beq .Lt_pwm
.Lt_write_freq:
    ldy VREG
    lda FREQ
    jsr sfx_write
    iny
    lda FREQ+1
    jsr sfx_write
.Lt_pwm:
    ; ---- pulse-width motion on pulse voices (or every lane when flagged)
    ldy PROG
    lda sfx_programs+G_PWM_RATE,y
    beq .Lt_gate
    lda WAVE
    and #$40
    bne .Lt_do_pwm
    lda ACCENT
    and #ACCENT_PWM_ALL
    beq .Lt_gate
.Lt_do_pwm:
    lda sfx_programs+G_PWM_RATE,y
    ldx AGE
    jsr .Lmul8
    sta T
    lda SID
    lsr
    bcc .Lt_phase
    lda T
    clc
    adc #37
    sta T
.Lt_phase:
    lda T
    bpl .Lt_triangle
    eor #$ff
.Lt_triangle:
    sta T
    lda #0
    sta T+1
    asl T
    rol T+1
    ldy PROG
    lda sfx_programs+G_PWM_DEPTH,y
    cmp #1
    beq .Lt_depth_done
    asl T
    rol T+1
    asl T
    rol T+1
.Lt_depth_done:
    ; pulse width = authored high byte * 256 + the triangle motion
    lda VOICE
    clc
    adc #P_PULSE
    tay
    lda (PATCH),y
    clc
    adc T+1
    sta T+1
    lda VREG
    clc
    adc #2
    tay
    lda T
    jsr sfx_write
    iny
    lda T+1
    jsr sfx_write
.Lt_gate:
    ; ---- gate stays high before the release tick, except on the retrigger tick
    ldx PROG
    lda sfx_programs+G_RETRIGGER,x
    sta T
    lda WAVE
    ldy AGE
    cpy RELEASE
    bcs .Lt_gate_value
    cpy T
    beq .Lt_gate_value
    ora #1
.Lt_gate_value:
    sta WAVE
    lda VREG
    clc
    adc #4
    tay
    lda WAVE
    jsr sfx_write
    lda VREG
    clc
    adc #7
    sta VREG
    inc VOICE
    lda VOICE
    cmp #3
    beq .Lt_next_sid
    jmp .Lt_voice
.Lt_next_sid:
    lda INST
    clc
    adc #INST_SIZE
    tax
    lda SIDBASE
    clc
    adc #32
    sta SIDBASE
    inc SID
    lda SID
    cmp #4
    beq .Lt_events
    jmp .Lt_sid
    ; ---- admit queued events, strongest first
.Lt_events:
    lda #0
    sta ADMITTED
.Lt_event_loop:
    lda EFFECTS+EFX_COUNT
    bne .Lt_select
    ldx #7
.Lt_restore:
    pla
    sta __rc20,x
    dex
    bpl .Lt_restore
    rts
.Lt_select:
    lda #0
    sta BESTIDX
    lda #1
    sta I
.Lt_best_loop:
    lda I
    cmp EFFECTS+EFX_COUNT
    bcs .Lt_best_done
    asl
    tay
    ldx EFFECTS+EFX_EVENTS,y
    lda sfx_priorities,x
    sta T
    lda BESTIDX
    asl
    tay
    ldx EFFECTS+EFX_EVENTS,y
    lda sfx_priorities,x
    cmp T
    bcs .Lt_best_next
    lda I
    sta BESTIDX
.Lt_best_next:
    inc I
    bne .Lt_best_loop
.Lt_best_done:
    lda BESTIDX
    asl
    tay
    lda EFFECTS+EFX_EVENTS,y
    sta EV_EFFECT
    lda EFFECTS+EFX_EVENTS+1,y
    sta EV_PAN
    lda BESTIDX
    jsr .Lremove_event
    jsr .Lstart
    ora ADMITTED
    sta ADMITTED
    jmp .Lt_event_loop

; PATCH = sfx_patches + Y * 25. Preserves X.
.Lpatch_pointer:
    lda .Lpatch_lo,y
    clc
    adc #<sfx_patches
    sta PATCH
    lda .Lpatch_hi,y
    adc #>sfx_patches
    sta PATCH+1
    rts

; T = A * 8 (an eight-bit cutoff as its eleven-bit register value). Preserves X.
.Lcutoff_target:
    sta T
    lda #0
    sta T+1
    asl T
    rol T+1
    asl T
    rol T+1
    asl T
    rol T+1
    rts

; T = FREQ >> Y (Y >= 1). Preserves X.
.Lshifted:
    lda FREQ
    sta T
    lda FREQ+1
    sta T+1
.Lshift_loop:
    lsr T+1
    ror T
    dey
    bne .Lshift_loop
    rts

; FREQ += T, saturating at $FFFF. Preserves X and Y.
.Lfreq_add_saturating:
    clc
    lda FREQ
    adc T
    sta FREQ
    lda FREQ+1
    adc T+1
    sta FREQ+1
    bcc .Lfreq_add_done
    lda #$ff
    sta FREQ
    sta FREQ+1
.Lfreq_add_done:
    rts

; FREQ -= T (T never exceeds FREQ). Preserves X and Y.
.Lfreq_subtract:
    sec
    lda FREQ
    sbc T
    sta FREQ
    lda FREQ+1
    sbc T+1
    sta FREQ+1
    rts

; A = low byte of A * X.
.Lmul8:
    sta T
    stx T+1
    lda #0
    ldx #8
.Lmul_loop:
    lsr T+1
    bcc .Lmul_skip
    clc
    adc T
.Lmul_skip:
    asl T
    dex
    bne .Lmul_loop
    rts

; Registers 21 and 22 of the current chip from the 11-bit cutoff in FREQ.
; Preserves X; FREQ is consumed.
.Lcutoff_write:
    lda SIDBASE
    ora #21
    tay
    lda FREQ
    and #7
    jsr sfx_write
    lsr FREQ+1
    ror FREQ
    lsr FREQ+1
    ror FREQ
    lsr FREQ+1
    ror FREQ
    iny
    lda FREQ
    jmp sfx_write

; Drop event A from the IRQ queue (head 0), shifting later events down and
; leaving the retired slot's bytes untouched, as the Rust reference does.
.Lremove_event:
    dec EFFECTS+EFX_COUNT
    asl
    tay
    lda EFFECTS+EFX_COUNT
    asl
    sta T
.Lremove_loop:
    cpy T
    bcs .Lremove_done
    lda EFFECTS+EFX_EVENTS+2,y
    sta EFFECTS+EFX_EVENTS,y
    lda EFFECTS+EFX_EVENTS+3,y
    sta EFFECTS+EFX_EVENTS+1,y
    iny
    iny
    bne .Lremove_loop
.Lremove_done:
    rts

; Allocate chips for EV_EFFECT/EV_PAN given ADMITTED; returns the chip mask
; in A (zero when nothing was free or weaker). Mirrors EffectsEngine::start.
.Lstart:
    ldx EV_EFFECT
    lda #3
    cpx #EFFECT_BOMB
    beq .Ls_lane_set
    cpx #EFFECT_BOSS_EXPLOSION
    beq .Ls_lane_set
    cpx #EFFECT_PLAYER_DEATH
    beq .Ls_lane_set
    lda EV_PAN
    jsr sfx_lane
.Ls_lane_set:
    sta LANE
    ; a repeated bomb replaces its own pair, leaving the overlap pair free
    lda #0
    sta RESTART
    cpx #EFFECT_BOMB
    bne .Ls_restart_done
    lda EFFECTS+I_EFFECT
    cmp #EFFECT_BOMB
    bne .Ls_restart_right
    lda EFFECTS+I_MASK
    sta RESTART
.Ls_restart_right:
    lda EFFECTS+I_EFFECT+2*INST_SIZE
    cmp #EFFECT_BOMB
    bne .Ls_restart_done
    lda EFFECTS+I_MASK+2*INST_SIZE
    ora RESTART
    sta RESTART
.Ls_restart_done:
    lda #0
    sta BEST
    sta VICTIMS
    sta BESTAGE
    sta CAND
    lda #$ff
    sta BESTCOST
    lda sfx_priorities,x
    sta EV_PRIO
    lda #2
    ldx LANE
    cpx #3
    bne .Ls_ncand
    lda #4
.Ls_ncand:
    sta NCAND
.Ls_cand_loop:
    lda CAND
    cmp NCAND
    bcc .Ls_candidate
    jmp .Ls_cand_done
.Ls_candidate:
    ; n = (candidate + rotation) & 3 selects the pair or the side's chip
    lda CAND
    clc
    adc EFFECTS+EFX_ROTATION
    and #3
    tax
    lda LANE
    cmp #3
    beq .Ls_mask_pair
    txa
    and #1
    beq .Ls_mask_first
    lda LANE
    asl
    asl
    bne .Ls_mask_set
.Ls_mask_first:
    lda LANE
    bne .Ls_mask_set
.Ls_mask_pair:
    lda .Lpair_mask,x
.Ls_mask_set:
    sta MASK
    lda RESTART
    beq .Ls_full
    cmp MASK
    beq .Ls_full
    jmp .Ls_next_cand
.Ls_full:
    ; full = mask plus every group that owns a chip in it
    lda MASK
    sta FULL
    sta BITS
    ldx #0
.Ls_full_loop:
    lsr BITS
    bcc .Ls_full_next
    lda EFFECTS+I_MASK,x
    ora FULL
    sta FULL
.Ls_full_next:
    txa
    clc
    adc #INST_SIZE
    tax
    cpx #INST_END
    bne .Ls_full_loop
    lda FULL
    and ADMITTED
    beq .Ls_occupants
    jmp .Ls_next_cand
.Ls_occupants:
    lda #0
    sta HIGHEST
    sta OCCUPIED
    lda #$ff
    sta YOUNGEST
    lda FULL
    sta BITS
    ldx #0
.Ls_occ_loop:
    lsr BITS
    bcc .Ls_occ_next
    lda EFFECTS+I_MASK,x
    beq .Ls_occ_next
    ldy EFFECTS+I_EFFECT,x
    lda sfx_priorities,y
    sta PRIO
    cmp EV_PRIO
    beq .Ls_occ_gun
    bcs .Ls_next_cand
.Ls_occ_gun:
    ; a gun may not retrigger its own first two ticks
    lda EV_EFFECT
    cmp #2
    bcs .Ls_occ_count
    cmp EFFECTS+I_EFFECT,x
    bne .Ls_occ_count
    lda EFFECTS+I_AGE,x
    cmp #2
    bcc .Ls_next_cand
.Ls_occ_count:
    lda PRIO
    cmp HIGHEST
    bcc .Ls_occ_occupied
    sta HIGHEST
.Ls_occ_occupied:
    inc OCCUPIED
    lda EFFECTS+I_AGE,x
    cmp YOUNGEST
    bcs .Ls_occ_next
    sta YOUNGEST
.Ls_occ_next:
    txa
    clc
    adc #INST_SIZE
    tax
    cpx #INST_END
    bne .Ls_occ_loop
    ; cost = highest * 8 + occupied; prefer cheaper, then the youngest victims
    lda HIGHEST
    asl
    asl
    asl
    adc OCCUPIED
    sta COST
    cmp BESTCOST
    bcc .Ls_take
    bne .Ls_next_cand
    lda YOUNGEST
    cmp BESTAGE
    beq .Ls_next_cand
    bcc .Ls_next_cand
.Ls_take:
    lda COST
    sta BESTCOST
    lda YOUNGEST
    sta BESTAGE
    lda MASK
    sta BEST
    lda FULL
    sta VICTIMS
    lda COST
    beq .Ls_cand_done
.Ls_next_cand:
    inc CAND
    jmp .Ls_cand_loop
.Ls_cand_done:
    lda BEST
    bne .Ls_allocate
    rts
.Ls_allocate:
    lda VICTIMS
    jsr sfx_stop
    ldy EV_EFFECT
    jsr .Lpatch_pointer
    lda #0
    sta SID
    sta SIDBASE
    sta INST
.Ls_sid_loop:
    ldy SID
    lda .Lsid_bit,y
    and BEST
    bne .Ls_chip
    jmp .Ls_sid_next
.Ls_chip:
    lda #0
    sta VOICE
    lda SIDBASE
    sta VREG
.Ls_voice:
    ; pitch = FREQUENCIES[note][standard], detuned by 1/512 on right chips
    lda VOICE
    clc
    adc #P_NOTES
    tay
    lda (PATCH),y
    asl
    asl
    ldy EFFECTS+EFX_STANDARD
    beq .Ls_note_index
    adc #2
.Ls_note_index:
    tay
    lda sfx_frequencies,y
    sta FREQ
    lda sfx_frequencies+1,y
    sta FREQ+1
    lda SID
    lsr
    bcc .Ls_pitch_done
    lda FREQ+1
    lsr
    clc
    adc FREQ
    sta FREQ
    bcc .Ls_pitch_done
    inc FREQ+1
.Ls_pitch_done:
    lda INST
    clc
    adc VOICE
    adc VOICE
    tax
    lda FREQ
    sta EFFECTS+I_FREQ,x
    lda FREQ+1
    sta EFFECTS+I_FREQ+1,x
    ldy VREG
    lda FREQ
    jsr sfx_write
    iny
    lda FREQ+1
    jsr sfx_write
    ; pulse width: the authored high byte, low byte zero
    lda VOICE
    clc
    adc #P_PULSE
    tay
    lda (PATCH),y
    sta T
    ldy VREG
    iny
    iny
    lda #0
    jsr sfx_write
    iny
    lda T
    jsr sfx_write
    lda VOICE
    clc
    adc #P_AD
    tay
    lda (PATCH),y
    sta T
    lda VREG
    clc
    adc #5
    tay
    lda T
    jsr sfx_write
    lda VOICE
    clc
    adc #P_SR
    tay
    lda (PATCH),y
    sta T
    lda VREG
    clc
    adc #6
    tay
    lda T
    jsr sfx_write
    ; a complete gate-low tick precedes the attack
    lda VOICE
    clc
    adc #P_WAVES
    tay
    lda (PATCH),y
    sta T
    lda VREG
    clc
    adc #4
    tay
    lda T
    jsr sfx_write
    lda VREG
    clc
    adc #7
    sta VREG
    inc VOICE
    lda VOICE
    cmp #3
    beq .Ls_filter
    jmp .Ls_voice
.Ls_filter:
    ; the initial cutoff, kept in the instance before its register writes
    ldy #P_CUTOFF
    lda (PATCH),y
    jsr .Lcutoff_target
    ldx INST
    lda T
    sta EFFECTS+I_CUTOFF,x
    sta FREQ
    lda T+1
    sta EFFECTS+I_CUTOFF+1,x
    sta FREQ+1
    jsr .Lcutoff_write
    ldy #P_FILTER
    lda (PATCH),y
    sta WAVE
    and #7
    sta T
    ldy #P_RESONANCE
    lda (PATCH),y
    and #$f0
    ora T
    sta T
    lda SIDBASE
    ora #23
    tay
    lda T
    jsr sfx_write
    ; chip volume: full when the patch has a punch, else the mix level
    ldy #P_LATE_RES
    lda (PATCH),y
    and #15
    beq .Ls_mix_volume
    lda #PUNCH_VOLUME
    bne .Ls_volume
.Ls_mix_volume:
    lda #VOLUME
.Ls_volume:
    sta T
    lda WAVE
    and #$f0
    ora T
    sta T
    lda SIDBASE
    ora #24
    tay
    lda T
    jsr sfx_write
    ; the instance: stereo right chips take the accent program
    ldx INST
    lda BEST
    sta EFFECTS+I_MASK,x
    lda EV_EFFECT
    sta EFFECTS+I_EFFECT,x
    lda #0
    sta EFFECTS+I_AGE,x
    ldy #P_DURATION
    lda (PATCH),y
    sta EFFECTS+I_REMAIN,x
    ldy #P_PROGRAMS
    lda (PATCH),y
    sta T
    lda LANE
    cmp #3
    bne .Ls_primary
    lda SID
    lsr
    bcc .Ls_primary
    lda T
    lsr
    lsr
    lsr
    lsr
    bpl .Ls_program
.Ls_primary:
    lda T
    and #15
.Ls_program:
    sta EFFECTS+I_PROGRAM,x
.Ls_sid_next:
    lda INST
    clc
    adc #INST_SIZE
    sta INST
    lda SIDBASE
    clc
    adc #32
    sta SIDBASE
    inc SID
    lda SID
    cmp #4
    beq .Ls_rotate
    jmp .Ls_sid_loop
.Ls_rotate:
    lda EFFECTS+EFX_ROTATION
    clc
    adc #1
    and #3
    sta EFFECTS+EFX_ROTATION
    lda BEST
    rts

; Patch byte offsets effect * 25, program byte offsets program * 11, the
; four stereo pair masks (n + 1) * 3 and chip bits.
.Lpatch_lo:
.irp k,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20
    .byte (\k * PATCH_SIZE) & 255
.endr
.Lpatch_hi:
.irp k,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20
    .byte (\k * PATCH_SIZE) >> 8
.endr
.Lprog_off:
.irp k,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15
    .byte \k * PROGRAM_SIZE
.endr
.Lpair_mask:
    .byte 3, 6, 9, 12
.Lsid_bit:
    .byte 1, 2, 4, 8
.size fast_effects_tick, . - fast_effects_tick

; A = value, Y = (chip << 5) | register. Writes the hardware only when the
; cached register differs. Preserves A, X and Y.
.section .text.sfx_write,"ax",@progbits
.globl sfx_write
.type sfx_write,@function
sfx_write:
    cmp EFFECTS+EFX_REGS,y
    beq .Lw_same
    sta EFFECTS+EFX_REGS,y
    sta $d400,y
.Lw_same:
    rts
.size sfx_write, . - sfx_write

; A = chip mask. Gate every voice low, clear the filter, restore the volume
; and retire the instances. Preserves X; uses STOP_* scratch only.
.section .text.sfx_stop,"ax",@progbits
.globl sfx_stop
.type sfx_stop,@function
sfx_stop:
    sta STOP_MASK
    stx STOP_X
    lda #0
    sta STOP_BASE
    tax
.Lstop_sid:
    lsr STOP_MASK
    bcc .Lstop_next
    lda #0
    sta STOP_I
.Lstop_reg:
    ldy STOP_I
    lda .Lstop_values,y
    pha
    lda .Lstop_regs,y
    ora STOP_BASE
    tay
    pla
    jsr sfx_write
    inc STOP_I
    lda STOP_I
    cmp #7
    bne .Lstop_reg
    lda #0
    sta EFFECTS+I_MASK,x
.Lstop_next:
    lda STOP_BASE
    clc
    adc #32
    sta STOP_BASE
    txa
    clc
    adc #INST_SIZE
    tax
    cpx #INST_END
    bne .Lstop_sid
    ldx STOP_X
    rts
.Lstop_regs:
    .byte 4, 11, 18, 21, 22, 23, 24
.Lstop_values:
    .byte 0, 0, 0, 0, 0, 0, VOLUME
.size sfx_stop, . - sfx_stop

; A = pan (i8) -> lane in A: 1 left (below -32), 2 right (above 32), else 3.
; Preserves X and Y.
.section .text.sfx_lane,"ax",@progbits
.globl sfx_lane
.type sfx_lane,@function
sfx_lane:
    eor #$80
    cmp #96
    bcc .Llane_left
    cmp #161
    bcs .Llane_right
    lda #3
    rts
.Llane_left:
    lda #1
    rts
.Llane_right:
    lda #2
    rts
.size sfx_lane, . - sfx_lane
