; void fast_project(const Vec3 *point, int16_t out[2])
; C ABI: point rc2/3 (x, y, z as little-endian i16), out rc4/5.
; The native body of projection::project_reference, exact to the host:
;   d = z; e = 0; while d > 255 { d >>= 1; e += 1 }; while d < 128 { d <<= 1; e -= 1 }
;   x' = clamp(80 + ((x * PROJECT_X[d - 128]) >> (15 + e)), -8191, 8191)
;   y' = clamp(100 + ((-y * PROJECT_Y[d - 128]) >> (15 + e)), -8191, 8191)
; with z = 0 giving (80, 100). The signed 16x16 product comes from four
; unsigned partials of the resident fast_mul8 (quarter-square table at $C000)
; with the sign applied afterwards; the shift drops whole bytes first so at
; most seven single-bit shifts run. Scratch at $9140-$9157 is free RAM that
; no interrupt touches. Projected points lie at or behind the near plane
; (z >= 8), so e >= -4 and every intermediate fits the host's i32.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.set PJ_P, $9140
.set PJ_E, $9146
.set PJ_IDX, $9147
.set PJ_U, $9148
.set PJ_MAG, $914a
.set PJ_NEG, $914c
.set PJ_M, $914d
.set PJ_T, $9151
.set PJ_OUTI, $9154

.section .phase.geometry,"ax",@progbits
.globl fast_project
.type fast_project,@function
fast_project:
    ldy #5
.Lpj_copy:
    lda (__rc2),y
    sta PJ_P,y
    dey
    bpl .Lpj_copy
    lda PJ_P+4
    ora PJ_P+5
    bne .Lpj_have_depth
    jmp .Lpj_zero_depth
.Lpj_have_depth:
    lda #0
    sta PJ_E
    lda PJ_P+5
    beq .Lpj_normalise_up
.Lpj_normalise_down:
    lsr PJ_P+5
    ror PJ_P+4
    inc PJ_E
    lda PJ_P+5
    bne .Lpj_normalise_down
    beq .Lpj_normalised
.Lpj_normalise_up:
    lda PJ_P+4
    bmi .Lpj_normalised
    asl PJ_P+4
    dec PJ_E
    jmp .Lpj_normalise_up
.Lpj_normalised:
    lda PJ_P+4
    and #$7f
    asl
    sta PJ_IDX
    tax
    ; X axis.
    lda PROJECT_X,x
    sta PJ_U
    lda PROJECT_X+1,x
    sta PJ_U+1
    lda PJ_P
    sta PJ_MAG
    lda PJ_P+1
    sta PJ_MAG+1
    sta PJ_NEG
    bpl .Lpj_x_product
    jsr .Lpj_negate_mag
.Lpj_x_product:
    jsr .Lpj_multiply
    jsr .Lpj_scale
    lda #80
    ldy #0
    jsr .Lpj_finish
    ; Y axis: the product takes -y so the shift rounds like the host.
    ldx PJ_IDX
    lda PROJECT_Y,x
    sta PJ_U
    lda PROJECT_Y+1,x
    sta PJ_U+1
    lda PJ_P+2
    sta PJ_MAG
    lda PJ_P+3
    sta PJ_MAG+1
    bmi .Lpj_y_negative
    ; y >= 0: -y is negative (or zero).
    lda #$80
    sta PJ_NEG
    bne .Lpj_y_product
.Lpj_y_negative:
    jsr .Lpj_negate_mag
    lda #0
    sta PJ_NEG
.Lpj_y_product:
    jsr .Lpj_multiply
    jsr .Lpj_scale
    lda #100
    ldy #2
    jmp .Lpj_finish
.Lpj_zero_depth:
    ldy #0
    lda #80
    sta (__rc4),y
    iny
    lda #0
    sta (__rc4),y
    iny
    lda #100
    sta (__rc4),y
    iny
    lda #0
    sta (__rc4),y
    rts

; M = sign * (MAG * U): four unsigned partials; U's high byte is never zero.
.Lpj_multiply:
    ldx PJ_MAG
    ldy PJ_U
    jsr fast_mul8
    sta PJ_M
    stx PJ_M+1
    lda #0
    sta PJ_M+2
    sta PJ_M+3
    ldx PJ_MAG
    ldy PJ_U+1
    jsr fast_mul8
    jsr .Lpj_add_middle
    lda PJ_MAG+1
    beq .Lpj_multiply_sign
    tax
    ldy PJ_U
    jsr fast_mul8
    jsr .Lpj_add_middle
    ldx PJ_MAG+1
    ldy PJ_U+1
    jsr fast_mul8
    clc
    adc PJ_M+2
    sta PJ_M+2
    txa
    adc PJ_M+3
    sta PJ_M+3
.Lpj_multiply_sign:
    bit PJ_NEG
    bpl .Lpj_multiply_done
    sec
    lda #0
    sbc PJ_M
    sta PJ_M
    lda #0
    sbc PJ_M+1
    sta PJ_M+1
    lda #0
    sbc PJ_M+2
    sta PJ_M+2
    lda #0
    sbc PJ_M+3
    sta PJ_M+3
.Lpj_multiply_done:
    rts
; Add a partial product (A low, X high) at byte 1 of M.
.Lpj_add_middle:
    clc
    adc PJ_M+1
    sta PJ_M+1
    txa
    adc PJ_M+2
    sta PJ_M+2
    bcc .Lpj_add_middle_done
    inc PJ_M+3
.Lpj_add_middle_done:
    rts
.Lpj_negate_mag:
    sec
    lda #0
    sbc PJ_MAG
    sta PJ_MAG
    lda #0
    sbc PJ_MAG+1
    sta PJ_MAG+1
    rts
; T (24-bit) = M >> (15 + e), arithmetic. For e >= 1 the result is the high
; word shifted right e - 1 times; otherwise bytes 1-3 shifted right 7 + e.
.Lpj_scale:
    lda PJ_E
    bmi .Lpj_scale_low
    beq .Lpj_scale_low
    lda PJ_M+2
    sta PJ_T
    lda PJ_M+3
    sta PJ_T+1
    bmi .Lpj_scale_high_negative
    lda #0
    beq .Lpj_scale_high_sign
.Lpj_scale_high_negative:
    lda #$ff
.Lpj_scale_high_sign:
    sta PJ_T+2
    ldx PJ_E
    dex
    beq .Lpj_scale_done
.Lpj_scale_high_loop:
    lda PJ_T+1
    cmp #$80
    ror PJ_T+1
    ror PJ_T
    dex
    bne .Lpj_scale_high_loop
    rts
.Lpj_scale_low:
    lda PJ_M+1
    sta PJ_T
    lda PJ_M+2
    sta PJ_T+1
    lda PJ_M+3
    sta PJ_T+2
    lda PJ_E
    clc
    adc #7
    beq .Lpj_scale_done
    tax
.Lpj_scale_low_loop:
    lda PJ_T+2
    cmp #$80
    ror PJ_T+2
    ror PJ_T+1
    ror PJ_T
    dex
    bne .Lpj_scale_low_loop
.Lpj_scale_done:
    rts
; out[Y] = clamp(T + A, -8191, 8191); A = offset, Y = output byte index.
.Lpj_finish:
    sty PJ_OUTI
    clc
    adc PJ_T
    sta PJ_T
    bcc .Lpj_finish_clamp
    inc PJ_T+1
    bne .Lpj_finish_clamp
    inc PJ_T+2
.Lpj_finish_clamp:
    lda PJ_T+2
    bmi .Lpj_finish_negative
    bne .Lpj_finish_high
    lda PJ_T+1
    cmp #$20
    bcs .Lpj_finish_high
    lda PJ_T
    ldx PJ_T+1
    jmp .Lpj_finish_store
.Lpj_finish_negative:
    cmp #$ff
    bne .Lpj_finish_low
    lda PJ_T+1
    cmp #$e0
    bcc .Lpj_finish_low
    bne .Lpj_finish_in_range
    lda PJ_T
    beq .Lpj_finish_low
.Lpj_finish_in_range:
    lda PJ_T
    ldx PJ_T+1
    jmp .Lpj_finish_store
.Lpj_finish_high:
    lda #$ff
    ldx #$1f
    bne .Lpj_finish_store
.Lpj_finish_low:
    lda #$01
    ldx #$e0
.Lpj_finish_store:
    ldy PJ_OUTI
    sta (__rc4),y
    iny
    txa
    sta (__rc4),y
    rts
.size fast_project, .-fast_project
