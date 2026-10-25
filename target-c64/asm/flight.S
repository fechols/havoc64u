; void fast_flight_step(uint8_t bits, int8_t push, Vec3 *player, Flight *flight)
; C ABI: bits A, push X, player rc2/3, flight rc4/5. No result.
; The native body of Game::move_player (src/game/flight.rs step_reference):
; one 20 ms tick of the player's assisted flight model, all Q8.
;   player: x @0, y @2 (i16 world units; the Q8 position's high bytes)
;   flight: frac[2] @0 (low bytes), vel[2] @2 (Q8 units/tick), bank @6,
;           pitch @8 (Q8 of 256ths of a turn)
; Per axis: target = stick (RIGHT/LEFT ±768, UP/DOWN ±704, or ±1152/±1056
; while BOOST bit $20 is set) plus the unchanged push × 640 on x;
; the velocity moves 3/32 of the way to the target (the shifted
; magnitude, at least one unit while any error remains, so it settles
; exactly); then the wall: on the near side of the arena (sign of the
; position) v <= (EDGE + 7 - q) >> 3 or v >= -((EDGE + 7 + q) >> 3) with
; EDGE 25600 (x) / 16640 (y); q += v; player gets the sign-extended high
; byte, frac the low byte. The bank target is -dv_x (units of 1/256 turn)
; unless |dv_x| <= 1, clamped to ±20, times 256; the pitch target is
; -(unboosted target_y << 2); both angles move 1/8 of their error per tick, with
; the same magnitude rule. Only caller-saved rc2..19 and A/X/Y change;
; no scratch RAM or stack frame beyond three JSR levels.
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
.set FL_PLAYER, __rc2
.set FL_FLIGHT, __rc4
.set FL_BITS, __rc6
.set FL_PUSH, __rc7             ; the push, then the wall's edge high byte
.set FL_EDGE_HI, __rc7
.set FL_T, __rc8                ; the axis target, then an angle target
.set FL_E, __rc10               ; error / delta / wall bound
.set FL_Q, __rc12               ; the Q8 position; delta_mag temporaries
.set FL_V, __rc14               ; the velocity
.set FL_NEG, __rc16             ; delta_mag: sign of the error; then dv low
.set FL_NZ, __rc17              ; delta_mag: error nonzero; then dv high
.set FL_DV, __rc16
.set FL_AX, __rc18              ; axis index 0 (x) or 1 (y)
.set FL_BOOST, __rc19           ; table bank offset: normal 0, boosted 4

.section .text.fast_flight_step,"ax",@progbits
.globl fast_flight_step
.type fast_flight_step,@function
fast_flight_step:
    sta FL_BITS
    stx FL_PUSH
    lsr
    lsr
    lsr
    and #4
    sta FL_BOOST
    ; ---- x target: RIGHT +768, LEFT -768 by stick bits 3:2, plus the push
    lda FL_BITS
    lsr
    lsr
    and #3
    ora FL_BOOST
    tax
    ldy FL_PUSH
    iny                         ; -1, 0, 1 -> 0, 1, 2
    clc
    lda .Lfl_x_targets_lo,x
    adc .Lfl_push_lo,y
    sta FL_T
    lda .Lfl_x_targets_hi,x
    adc .Lfl_push_hi,y
    sta FL_T+1
    lda #0
    sta FL_AX
    lda #$64                    ; EDGE_X + 7 = $6407
    sta FL_EDGE_HI
    jsr .Lfl_axis
    ; ---- bank target from dv_x: 0 while |dv| <= 1, else clamp(-dv, ±20) << 8
    lda FL_DV
    sta FL_E
    lda FL_DV+1
    sta FL_E+1
    jsr .Lfl_negate
    lda FL_E+1
    beq .Lfl_n_small_pos        ; n in 0..255
    bpl .Lfl_n_plus             ; n >= 256
    cmp #$ff
    bne .Lfl_n_minus            ; n <= -257
    lda FL_E                    ; n in -256..-1
    cmp #$ff
    beq .Lfl_n_zero             ; -1
    cmp #$ec
    bcs .Lfl_n_set              ; -20..-2
.Lfl_n_minus:
    lda #$ec
    bne .Lfl_n_set
.Lfl_n_small_pos:
    lda FL_E
    cmp #2
    bcc .Lfl_n_zero             ; 0 or 1
    cmp #21
    bcc .Lfl_n_set              ; 2..20
.Lfl_n_plus:
    lda #20
    bne .Lfl_n_set
.Lfl_n_zero:
    lda #0
.Lfl_n_set:
    sta FL_T+1
    lda #0
    sta FL_T
    ldy #6
    jsr .Lfl_angle
    ; ---- y target: UP +704, DOWN -704 by stick bits 1:0
    lda FL_BITS
    and #3
    ora FL_BOOST
    tax
    lda .Lfl_y_targets_lo,x
    sta FL_T
    lda .Lfl_y_targets_hi,x
    sta FL_T+1
    lda #1
    sta FL_AX
    lda #$41                    ; EDGE_Y + 7 = $4107
    sta FL_EDGE_HI
    jsr .Lfl_axis
    ; ---- pitch target uses the normal climb command, even while boosting
    lda FL_BITS
    and #3
    tax
    lda #0
    sta FL_T
    lda .Lfl_pitch_targets_hi,x
    sta FL_T+1
    ldy #8
    jmp .Lfl_angle

; One axis (FL_AX, FL_T target, FL_EDGE_HI): lagged velocity, wall,
; position; stores vel/frac/player and leaves dv in FL_DV. FL_T survives.
.Lfl_axis:
    lda FL_AX
    asl
    adc #2
    tay
    lda (FL_FLIGHT),y           ; velocity
    sta FL_V
    iny
    lda (FL_FLIGHT),y
    sta FL_V+1
    sec
    lda FL_T
    sbc FL_V
    sta FL_E
    lda FL_T+1
    sbc FL_V+1
    sta FL_E+1
    ldx #4
    jsr .Lfl_delta_mag          ; FL_E = |e| >> 4 (or 1)
    lda FL_E                    ; plus half of it: 3/32 of the error
    sta FL_Q
    lda FL_E+1
    sta FL_Q+1
    lsr FL_Q+1
    ror FL_Q
    clc
    lda FL_E
    adc FL_Q
    sta FL_E
    lda FL_E+1
    adc FL_Q+1
    sta FL_E+1
    jsr .Lfl_apply_sign
    clc
    lda FL_V
    adc FL_E
    sta FL_V
    lda FL_V+1
    adc FL_E+1
    sta FL_V+1
    ; the Q8 position: player's low byte over the fraction
    ldy FL_AX
    lda (FL_FLIGHT),y
    sta FL_Q
    lda FL_AX
    asl
    tay
    lda (FL_PLAYER),y
    sta FL_Q+1
    bmi .Lfl_wall_negative
    ; bound = (EDGE + 7 - q) >> 3; v = min(v, bound)
    sec
    lda #7
    sbc FL_Q
    sta FL_E
    lda FL_EDGE_HI
    sbc FL_Q+1
    sta FL_E+1
    jsr .Lfl_asr3
    sec
    lda FL_E
    sbc FL_V
    lda FL_E+1
    sbc FL_V+1
    bvc .Lfl_wall_min_sign
    eor #$80
.Lfl_wall_min_sign:
    bpl .Lfl_wall_done          ; bound - v >= 0
    bmi .Lfl_wall_clamp
.Lfl_wall_negative:
    ; bound = -((EDGE + 7 + q) >> 3); v = max(v, bound)
    clc
    lda #7
    adc FL_Q
    sta FL_E
    lda FL_EDGE_HI
    adc FL_Q+1
    sta FL_E+1
    jsr .Lfl_asr3
    jsr .Lfl_negate
    sec
    lda FL_V
    sbc FL_E
    lda FL_V+1
    sbc FL_E+1
    bvc .Lfl_wall_max_sign
    eor #$80
.Lfl_wall_max_sign:
    bpl .Lfl_wall_done          ; v - bound >= 0
.Lfl_wall_clamp:
    lda FL_E
    sta FL_V
    lda FL_E+1
    sta FL_V+1
.Lfl_wall_done:
    clc
    lda FL_Q
    adc FL_V
    sta FL_Q
    lda FL_Q+1
    adc FL_V+1
    sta FL_Q+1
    ; store: fraction, sign-extended coordinate, dv = v - old, velocity
    ldy FL_AX
    lda FL_Q
    sta (FL_FLIGHT),y
    lda FL_AX
    asl
    tay
    lda FL_Q+1
    sta (FL_PLAYER),y
    iny
    ldx #0
    ora #0
    bpl .Lfl_positive_coordinate
    dex
.Lfl_positive_coordinate:
    txa
    sta (FL_PLAYER),y
    lda FL_AX
    asl
    adc #2
    tay
    sec
    lda FL_V
    sbc (FL_FLIGHT),y
    sta FL_DV
    lda FL_V
    sta (FL_FLIGHT),y
    iny
    lda FL_V+1
    sbc (FL_FLIGHT),y
    sta FL_DV+1
    lda FL_V+1
    sta (FL_FLIGHT),y
    rts

; angle at flight offset Y (6 bank, 8 pitch) moves toward FL_T by 1/8 of
; the error (magnitude rule). Y is preserved across the delta routines.
.Lfl_angle:
    sec
    lda FL_T
    sbc (FL_FLIGHT),y
    sta FL_E
    iny
    lda FL_T+1
    sbc (FL_FLIGHT),y
    sta FL_E+1
    dey
    ldx #3
    jsr .Lfl_delta_mag
    jsr .Lfl_apply_sign
    clc
    lda (FL_FLIGHT),y
    adc FL_E
    sta (FL_FLIGHT),y
    iny
    lda (FL_FLIGHT),y
    adc FL_E+1
    sta (FL_FLIGHT),y
    rts

; FL_E = |FL_E| >> X, or 1 when that is zero but the error was not;
; FL_NEG keeps the error's sign (bit 7) for apply_sign. Preserves Y.
.Lfl_delta_mag:
    lda FL_E+1
    sta FL_NEG
    bpl .Lfl_magnitude
    jsr .Lfl_negate
.Lfl_magnitude:
    lda FL_E
    ora FL_E+1
    sta FL_NZ
.Lfl_shift:
    lsr FL_E+1
    ror FL_E
    dex
    bne .Lfl_shift
    lda FL_E
    ora FL_E+1
    bne .Lfl_delta_done
    lda FL_NZ
    beq .Lfl_delta_done
    inc FL_E
.Lfl_delta_done:
    rts

; negate FL_E when the error that produced it was negative
.Lfl_apply_sign:
    bit FL_NEG
    bmi .Lfl_negate
    rts

; FL_E = -FL_E
.Lfl_negate:
    sec
    lda #0
    sbc FL_E
    sta FL_E
    lda #0
    sbc FL_E+1
    sta FL_E+1
    rts

; FL_E >>= 3, arithmetic
.Lfl_asr3:
    ldx #3
.Lfl_asr3_loop:
    lda FL_E+1
    cmp #$80
    ror FL_E+1
    ror FL_E
    dex
    bne .Lfl_asr3_loop
    rts

; stick targets by bits 3:2 (x: none, LEFT, RIGHT, both) and 1:0 (y: none,
; UP, DOWN, both). The second bank of four entries is the boosted target.
; The push is indexed by direction + 1 and is never scaled by boost.
.Lfl_x_targets_lo:
    .byte $00, $00, $00, $00, $00, $80, $80, $00
.Lfl_x_targets_hi:
    .byte $00, $fd, $03, $00, $00, $fb, $04, $00
.Lfl_y_targets_lo:
    .byte $00, $c0, $40, $00, $00, $20, $e0, $00
.Lfl_y_targets_hi:
    .byte $00, $02, $fd, $00, $00, $04, $fb, $00
.Lfl_pitch_targets_hi:
    .byte $00, $f5, $0b, $00    ; normal climb command × -4; low byte is zero
.Lfl_push_lo:
    .byte $80, $00, $80
.Lfl_push_hi:
    .byte $fd, $00, $02
.size fast_flight_step, . - fast_flight_step
