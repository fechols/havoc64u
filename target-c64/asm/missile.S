; void fast_steer_missile(SteerArgs *args)
; C ABI: args rc2/3. The native body of Game::steer_missile with its
; missile_target search and missile_velocity guidance, for one homing missile
; per call. SteerArgs (repr(C), 19 bytes):
;    0 actors        *const Actor   the twelve 16-byte actor slots
;    2 specs         *const u8      the bank's enemy descriptors (8 bytes each)
;    4 shot          Vec3           the missile's position
;   10 bosses        *const BossInstance (ten-byte records: Vec3, left/right/core/flash)
;   12 enemy_count   u8             descriptors in the bank (kinds 1..count)
;   13 boss_count    u8             0..4; zero during the split pause
;   14 boss_shift    u8             geometry scale: 0 original, 2 medium, 3 small
;   15 dx, 16 dy     i8 (in/out)    the missile's lateral velocity
;   17 target        u8 (in/out)    0 none, 1..12 actors, 13..24 boss parts
;   18 rng           u8 (in/out)    right-shift LFSR, feedback $B8, seed $A5
; Keep an eligible target without drawing randomness. Otherwise count live
; candidates in slot/left turret/right turret/core order, choose a uniform
; rank and scan again for that candidate. An actor with kind 0 or hp 0 is
; skipped, as is one whose descriptor hp is 0 unless it is small debris
; (a kind past enemy_count has the default descriptor); its
; aim point is pos with y + center_y (10 for small debris) and closing speed
; 32 + approach_speed. Each live boss contributes its left/right turrets or
; its exposed core. Offsets (-/+130,30,-70) and (0,0,-200) divide by
; 1 << boss_shift, truncating toward zero, before adding the instance position.
; IDs are 13 + 3 * instance + part (left/right/core). A zero core disables
; the whole instance; closing is 40 when its z > 680, else 32. A candidate
; needs dz = z - shot.z > 0. No candidates or a single candidate consume no
; random draws. For larger pools, advance the 255-state LFSR and accept its
; high nibble only in 1..count when count <= 15, preserving ordinary targeting.
; Larger pools use its high five bits; every accepted rank appears eight times
; per full cycle. A zero input seed is repaired only when a draw is needed.
; Guidance: with no target the desired velocity is (0, 0); otherwise with
; d = max(dz, closing), v = clamp(clamp(error, -512, 512) * closing / d, -12,
; 12) per axis, the product taken as a wrapping i16 and the division truncating
; toward zero (the quotient is bounded, so a subtraction loop replaces it);
; dx += clamp(v - dx, -1, 1), likewise dy. Scratch at $9340-$937F is GEOMETRY's
; projection scratch, idle during SIM; no interrupt touches it.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.set MS_ACT, $9340
.set MS_SPEC, $9342
.set MS_PX, $9344
.set MS_PY, $9346
.set MS_PZ, $9348
.set MS_BOSSES, $934a
.set MS_EC, $934c
.set MS_BCOUNT, $934d
.set MS_SHIFT, $934e
.set MS_DX, $934f
.set MS_DY, $9350
.set MS_REMAIN, $9351
.set MS_PART, $9352
.set MS_TX, $9353
.set MS_TY, $9355
.set MS_TZ, $9357
.set MS_TC, $9359
.set MS_BEST, $935b
.set MS_BESTC, $9361
.set MS_PASS, $9363
.set MS_COUNT, $9364
.set MS_RANK, $9365
.set MS_FOUND, $9367
.set MS_DZ, $9368
.set MS_SLOT, $936c
.set MS_T, $936d
.set MS_E, $9371
.set MS_P, $9373
.set MS_NEG, $9377
.set MS_V, $9378
.set MS_ARGS, $937a
.set MS_LOCK, $937c
.set MS_RNG, $937d

.section .phase.sim.missile,"ax",@progbits
.globl fast_steer_missile
.type fast_steer_missile,@function
fast_steer_missile:
    lda __rc2
    sta MS_ARGS
    lda __rc3
    sta MS_ARGS+1
    ldy #16
.Lms_copy_args:
    lda (__rc2),y
    sta MS_ACT,y
    dey
    bpl .Lms_copy_args
    ldy #17
    lda (__rc2),y
    sta MS_LOCK
    iny
    lda (__rc2),y
    sta MS_RNG
    lda #0
    sta MS_FOUND
    sta MS_PASS
    sta MS_COUNT
.Lms_scan:
    lda #1
    sta MS_SLOT
    lda MS_ACT
    sta __rc4
    lda MS_ACT+1
    sta __rc5
.Lms_actor:
    ldy #13
    lda (__rc4),y
    bne .Lms_alive
    jmp .Lms_actor_next         ; hp 0
.Lms_alive:
    ldy #14
    lda (__rc4),y
    and #2
    sta MS_T                    ; small debris flag
    ldy #12
    lda (__rc4),y               ; kind
    bne .Lms_actor_kind
    jmp .Lms_actor_next
.Lms_actor_kind:
    cmp MS_EC
    beq .Lms_have_spec
    bcs .Lms_default_spec
.Lms_have_spec:
    ; descriptor = specs + 8 * (kind - 1)
    sec
    sbc #1
    asl
    asl
    asl
    clc
    adc MS_SPEC
    sta __rc2
    lda MS_SPEC+1
    adc #0
    sta __rc3
    ldy #0
    lda (__rc2),y               ; descriptor hp
    bne .Lms_spec_alive
    lda MS_T
    beq .Lms_actor_next
.Lms_spec_alive:
    lda MS_T
    bne .Lms_small
    ldy #1
    lda (__rc2),y               ; center_y (i8)
    jmp .Lms_center
.Lms_default_spec:
    lda MS_T
    beq .Lms_actor_next
.Lms_small:
    lda #10
.Lms_center:
    ; candidate: pos.x, pos.y + center, pos.z, closing 32 + approach_speed
    sta MS_T+1
    ldy #0
    lda (__rc4),y
    sta MS_TX
    iny
    lda (__rc4),y
    sta MS_TX+1
    iny
    lda (__rc4),y
    clc
    adc MS_T+1
    sta MS_TY
    iny
    lda (__rc4),y
    ldx MS_T+1
    bmi .Lms_center_negative
    adc #0
    jmp .Lms_center_added
.Lms_center_negative:
    adc #$ff
.Lms_center_added:
    sta MS_TY+1
    iny
    lda (__rc4),y
    sta MS_TZ
    iny
    lda (__rc4),y
    sta MS_TZ+1
    ldy #15
    lda (__rc4),y
    clc
    adc #32
    sta MS_TC
    lda #0
    adc #0
    sta MS_TC+1
    jsr .Lms_evaluate
.Lms_actor_next:
    lda MS_FOUND
    beq .Lms_actor_advance
    jmp .Lms_guide
.Lms_actor_advance:
    clc
    lda __rc4
    adc #16
    sta __rc4
    bcc .Lms_actor_count
    inc __rc5
.Lms_actor_count:
    inc MS_SLOT
    lda MS_SLOT
    cmp #13
    beq .Lms_boss
    jmp .Lms_actor
.Lms_boss:
    lda MS_BCOUNT
    sta MS_REMAIN
    bne .Lms_boss_start
    jmp .Lms_scan_done
.Lms_boss_start:
    lda MS_BOSSES
    sta __rc4
    lda MS_BOSSES+1
    sta __rc5
.Lms_boss_instance:
    lda #0
    sta MS_PART
    ; closing 40 above 680, else 32
    lda #32
    sta MS_TC
    lda #0
    sta MS_TC+1
    ldy #5
    lda (__rc4),y
    bmi .Lms_boss_closing
    cmp #>680
    bcc .Lms_boss_closing
    bne .Lms_boss_far
    dey
    lda (__rc4),y
    cmp #<(680+1)
    bcc .Lms_boss_closing
.Lms_boss_far:
    lda #40
    sta MS_TC
.Lms_boss_closing:
    ldy #8
    lda (__rc4),y              ; a dead core disables the complete instance
    bne .Lms_boss_live
    jmp .Lms_boss_next_part
.Lms_boss_live:
    lda MS_PART
    cmp #2
    beq .Lms_boss_core
    clc
    adc #6
    tay
    lda (__rc4),y              ; the selected side must still be alive
    bne .Lms_boss_position
    jmp .Lms_boss_next_part
.Lms_boss_core:
    ldy #6
    lda (__rc4),y
    iny
    ora (__rc4),y
    beq .Lms_boss_position
    jmp .Lms_boss_next_part
.Lms_boss_position:
    ldy #5
.Lms_boss_copy:
    lda (__rc4),y
    sta MS_TX,y
    dey
    bpl .Lms_boss_copy
    lda MS_PART
    cmp #2
    beq .Lms_boss_core_offset
    lda #130
    jsr .Lms_scaled_offset
    sta MS_T
    lda MS_PART
    bne .Lms_boss_right_offset
    sec
    lda MS_TX
    sbc MS_T
    sta MS_TX
    lda MS_TX+1
    sbc #0
    sta MS_TX+1
    jmp .Lms_boss_side_y
.Lms_boss_right_offset:
    clc
    lda MS_TX
    adc MS_T
    sta MS_TX
    lda MS_TX+1
    adc #0
    sta MS_TX+1
.Lms_boss_side_y:
    lda #30
    jsr .Lms_scaled_offset
    clc
    adc MS_TY
    sta MS_TY
    lda MS_TY+1
    adc #0
    sta MS_TY+1
    lda #70
    bne .Lms_boss_depth
.Lms_boss_core_offset:
    lda #200
.Lms_boss_depth:
    jsr .Lms_scaled_offset
    sta MS_T
    sec
    lda MS_TZ
    sbc MS_T
    sta MS_TZ
    lda MS_TZ+1
    sbc #0
    sta MS_TZ+1
    jsr .Lms_evaluate
.Lms_boss_next_part:
    lda MS_FOUND
    beq .Lms_boss_advance_part
    jmp .Lms_guide
.Lms_boss_advance_part:
    inc MS_SLOT
    inc MS_PART
    lda MS_PART
    cmp #3
    beq .Lms_boss_advance_instance
    jmp .Lms_boss_closing
.Lms_boss_advance_instance:
    clc
    lda __rc4
    adc #10
    sta __rc4
    bcc .Lms_boss_count
    inc __rc5
.Lms_boss_count:
    dec MS_REMAIN
    beq .Lms_scan_done
    jmp .Lms_boss_instance
.Lms_scan_done:
    lda MS_FOUND
    bne .Lms_guide
    lda #0
    sta MS_LOCK
    lda MS_COUNT
    beq .Lms_guide
    cmp #1
    beq .Lms_only_target
    lda MS_RNG
    bne .Lms_random
    lda #$a5
.Lms_random:
    lsr
    bcc .Lms_random_ready
    eor #$b8
.Lms_random_ready:
    sta MS_RNG
    lsr
    lsr
    lsr
    ldx MS_COUNT
    cpx #16
    bcs .Lms_random_sample
    lsr
.Lms_random_sample:
    cmp #0                      ; CPX above must not supply the zero flag
    beq .Lms_random_retry
    cmp MS_COUNT
    beq .Lms_rank_ready
    bcc .Lms_rank_ready
.Lms_random_retry:
    lda MS_RNG
    jmp .Lms_random
.Lms_only_target:
    lda #1
.Lms_rank_ready:
    sec
    sbc #1
    sta MS_RANK
    lda #1
    sta MS_PASS
    jmp .Lms_scan
.Lms_guide:
    ; desired velocity per axis
    lda MS_FOUND
    bne .Lms_aim
    lda #0
    sta MS_V
    sta MS_V+1
    jmp .Lms_apply
.Lms_aim:
    ; d = max(dz, closing), dz = best.z - shot.z (> 0)
    sec
    lda MS_BEST+4
    sbc MS_PZ
    sta MS_DZ
    lda MS_BEST+5
    sbc MS_PZ+1
    sta MS_DZ+1
    sec
    lda MS_DZ
    sbc MS_BESTC
    lda MS_DZ+1
    sbc MS_BESTC+1
    bcs .Lms_d_ready            ; dz >= closing (both nonnegative)
    lda MS_BESTC
    sta MS_DZ
    lda MS_BESTC+1
    sta MS_DZ+1
.Lms_d_ready:
    ; x axis
    sec
    lda MS_BEST
    sbc MS_PX
    sta MS_E
    lda MS_BEST+1
    sbc MS_PX+1
    sta MS_E+1
    jsr .Lms_velocity
    sta MS_V
    ; y axis
    sec
    lda MS_BEST+2
    sbc MS_PY
    sta MS_E
    lda MS_BEST+3
    sbc MS_PY+1
    sta MS_E+1
    jsr .Lms_velocity
    sta MS_V+1
.Lms_apply:
    ; dx += clamp(v - dx, -1, 1)
    lda MS_V
    ldx MS_DX
    jsr .Lms_step
    sta MS_DX
    lda MS_V+1
    ldx MS_DY
    jsr .Lms_step
    sta MS_DY
    ; write velocity, target identity and RNG state back through args
    lda MS_ARGS
    sta __rc2
    lda MS_ARGS+1
    sta __rc3
    ldy #15
    lda MS_DX
    sta (__rc2),y
    iny
    lda MS_DY
    sta (__rc2),y
    iny
    lda MS_LOCK
    sta (__rc2),y
    iny
    lda MS_RNG
    sta (__rc2),y
    rts

; Validate/count candidates on pass 0, keeping an eligible lock immediately;
; otherwise select the randomly chosen rank on pass 1.
.Lms_evaluate:
    sec
    lda MS_TZ
    sbc MS_PZ
    sta MS_DZ
    lda MS_TZ+1
    sbc MS_PZ+1
    sta MS_DZ+1
    ; dz > 0: positive and nonzero
    bmi .Lms_evaluate_done
    ora MS_DZ
    beq .Lms_evaluate_done
    lda MS_PASS
    bne .Lms_select_rank
    inc MS_COUNT
    lda MS_SLOT
    cmp MS_LOCK
    beq .Lms_take
    rts
.Lms_select_rank:
    lda MS_RANK
    beq .Lms_take
    dec MS_RANK
    rts
.Lms_take:
    ldx #5
.Lms_take_copy:
    lda MS_TX,x
    sta MS_BEST,x
    dex
    bpl .Lms_take_copy
    lda MS_TC
    sta MS_BESTC
    lda MS_TC+1
    sta MS_BESTC+1
    lda MS_SLOT
    sta MS_LOCK
    lda #1
    sta MS_FOUND
.Lms_evaluate_done:
    rts

; Divide a positive offset magnitude by 1 << scale. Applying the sign later
; gives truncation toward zero for negative offsets as well.
.Lms_scaled_offset:
    ldx MS_SHIFT
    beq .Lms_scaled_done
.Lms_scaled_loop:
    lsr
    dex
    bne .Lms_scaled_loop
.Lms_scaled_done:
    rts

; A = clamp(clamp(MS_E, -512, 512) * closing / MS_DZ, -12, 12) as a signed byte.
.Lms_velocity:
    ; clamp the error to +-512
    lda MS_E+1
    bmi .Lms_error_negative
    cmp #2
    bcc .Lms_error_clamped
    bne .Lms_error_high
    lda MS_E
    beq .Lms_error_clamped
.Lms_error_high:
    lda #<512
    sta MS_E
    lda #>512
    sta MS_E+1
    jmp .Lms_error_clamped
.Lms_error_negative:
    cmp #$fe
    bcs .Lms_error_clamped      ; -512..-1 ($FE00..$FFFF)
    lda #$00                    ; -512
    sta MS_E
    lda #$fe
    sta MS_E+1
.Lms_error_clamped:
    ; |e| (0..512) and its sign
    lda MS_E+1
    sta MS_NEG
    bpl .Lms_magnitude
    sec
    lda #0
    sbc MS_E
    sta MS_E
    lda #0
    sbc MS_E+1
    sta MS_E+1
.Lms_magnitude:
    ; p = low 16 bits of |e| * closing: |e| = e1:e0 (e1 <= 2), closing = c1:c0
    ; (c1 <= 1): e0*c0 + (e0*c1 + e1*c0) << 8, the e1*c1 term lands beyond
    ; bit 15 (and e1 c1 <= 2 << 16), as does any carry: all dropped.
    ldx MS_E
    ldy MS_BESTC
    jsr fast_mul8
    sta MS_P
    stx MS_P+1
    lda MS_BESTC+1
    beq .Lms_no_c1
    ; e0 * c1 with c1 = 1: add e0 to the high byte
    clc
    lda MS_P+1
    adc MS_E
    sta MS_P+1
.Lms_no_c1:
    lda MS_E+1
    beq .Lms_product
    ; e1 * c0 for e1 = 1 or 2
    ldx MS_E+1
    ldy MS_BESTC
    jsr fast_mul8
    clc
    adc MS_P+1
    sta MS_P+1
.Lms_product:
    ; the wrapping i16 product: its sign is bit 15 after applying e's sign
    bit MS_NEG
    bpl .Lms_product_signed
    sec
    lda #0
    sbc MS_P
    sta MS_P
    lda #0
    sbc MS_P+1
    sta MS_P+1
.Lms_product_signed:
    ; q = trunc(p / d): sign from p, magnitude |p| / d
    lda MS_P+1
    sta MS_NEG
    bpl .Lms_quotient
    sec
    lda #0
    sbc MS_P
    sta MS_P
    lda #0
    sbc MS_P+1
    sta MS_P+1
.Lms_quotient:
    ; q = min(|p| / d, 12) by repeated subtraction: at most twelve rounds,
    ; so the loop is bounded for every d >= 1 and never forms 12 d
    ldx #0
.Lms_divide_loop:
    lda MS_P
    cmp MS_DZ
    lda MS_P+1
    sbc MS_DZ+1
    bcc .Lms_divided            ; |p| < d
    sec
    lda MS_P
    sbc MS_DZ
    sta MS_P
    lda MS_P+1
    sbc MS_DZ+1
    sta MS_P+1
    inx
    cpx #12
    bne .Lms_divide_loop
.Lms_divided:
    txa
    bit MS_NEG
    bpl .Lms_velocity_done
    eor #$ff
    clc
    adc #1
.Lms_velocity_done:
    rts

; A = X + clamp(A - X, -1, 1) for signed bytes within +-12.
.Lms_step:
    stx MS_T
    sec
    sbc MS_T
    beq .Lms_step_same
    bmi .Lms_step_down
    lda MS_T
    clc
    adc #1
    rts
.Lms_step_down:
    lda MS_T
    sec
    sbc #1
    rts
.Lms_step_same:
    lda MS_T
    rts
.size fast_steer_missile, .-fast_steer_missile
