; Persistent Q16 two-edge row walker. C ABI: job pointer in rc2/3.
; Job: head pointer, tail pointer, pixels pointer, u8 band_y, u8 paint.
; Head16: i32 long_x,long_step,short_x,short_step.
; Tail12: i32 lower_x,lower_step; u8 top,middle,bottom,color_flags.
; flags bit4: long is left. Caller owns sequential bands and ordered masks.
; rc2..13 and $77A0-$77AB are volatile; IRQ never accesses private scratch.
; $77AB is reset once per band, then retains whether a terrain-clear span was painted.
; Tail bit5 marks terrain-clear geometry; bit6 marks exterior terrain. Pixel bit6
; retains protected coverage through later ordinary foreground writes.
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
.set RR_BAND, $77a0
.set RR_PAINT, $77a1
.set RR_ROW, $77a2
.set RR_END, $77a3
.set RR_COLOR, $77a4
.set RR_MIDDLE, $77a5
.set RR_FLAGS, $77a6
.set RR_FIRST, $77a7
.set RR_LAST, $77a8
.set RR_BOUND, $77a9
.set RR_OFFSET, $77aa
.set RR_PLAYER_SEEN, $77ab

.section .phase.pixels.raster,"ax",@progbits
.globl fast_raster_rows
fast_raster_rows:
    ldy #0
    lda (__rc2),y
    sta __rc4
    iny
    lda (__rc2),y
    sta __rc5
    iny
    lda (__rc2),y
    sta __rc6
    iny
    lda (__rc2),y
    sta __rc7
    iny
    lda (__rc2),y
    sta __rc8
    iny
    lda (__rc2),y
    sta __rc9
    iny
    lda (__rc2),y
    sta RR_BAND
    sta RR_ROW
    clc
    adc #8
    sta RR_END
    iny
    lda (__rc2),y
    sta RR_PAINT
    ldy #8
    lda (__rc6),y
    cmp RR_ROW
    bcc .Lrr_top
    sta RR_ROW
.Lrr_top:
    iny
    lda (__rc6),y
    sta RR_MIDDLE
    iny
    lda (__rc6),y
    cmp RR_END
    bcs .Lrr_bottom
    sta RR_END
.Lrr_bottom:
    lda RR_ROW
    cmp RR_END
    bcc .Lrr_nonempty
    rts
.Lrr_nonempty:
    iny
    lda (__rc6),y
    sta RR_FLAGS
    and #15
    sta RR_COLOR
    sec
    lda RR_ROW
    sbc RR_BAND
    tax
    beq .Lrr_row
.Lrr_initial_row:
    clc
    lda __rc8
    adc #160
    sta __rc8
    bcc .Lrr_initial_no_carry
    inc __rc9
.Lrr_initial_no_carry:
    dex
    bne .Lrr_initial_row
.Lrr_row:
    lda RR_ROW
    cmp RR_MIDDLE
    bne .Lrr_span
    clc
    lda __rc4
    adc #8
    sta __rc12
    lda __rc5
    adc #0
    sta __rc13
    ldy #7
.Lrr_lower:
    lda (__rc6),y
    sta (__rc12),y
    dey
    bpl .Lrr_lower
.Lrr_span:
    lda RR_PAINT
    bne .Lrr_paint
    jmp .Lrr_advance
.Lrr_paint:
    lda RR_FLAGS
    and #16
    beq .Lrr_short_left
    ldy #0
    lda #8
    bne .Lrr_bounds
.Lrr_short_left:
    ldy #8
    lda #0
.Lrr_bounds:
    sta RR_OFFSET
    jsr .Lrr_clamp
    sta RR_FIRST
    ldy RR_OFFSET
    jsr .Lrr_clamp
    sta RR_LAST
    sec
    sbc RR_FIRST
    beq .Lrr_advance
    bcc .Lrr_advance
    tay
    clc
    lda __rc8
    adc RR_FIRST
    sta __rc10
    lda __rc9
    adc #0
    sta __rc11
    ; The span is painted and nonempty. No earlier branch may mark a band.
    lda RR_FLAGS
    and #$20
    beq .Lrr_ordinary
    lda #1
    sta RR_PLAYER_SEEN
    lda RR_COLOR
    ora #$40
    jmp .Lrr_fill
.Lrr_ordinary:
    lda RR_PLAYER_SEEN
    beq .Lrr_plain
    lda RR_FLAGS
    and #$40
    bne .Lrr_terrain
.Lrr_preserve:
    dey
    lda (__rc10),y
    and #$40
    ora RR_COLOR
    sta (__rc10),y
    cpy #0
    bne .Lrr_preserve
    jmp .Lrr_advance
.Lrr_terrain:
    dey
    lda (__rc10),y
    and #$40
    bne .Lrr_protected
    lda RR_COLOR
    sta (__rc10),y
.Lrr_protected:
    cpy #0
    bne .Lrr_terrain
    jmp .Lrr_advance
.Lrr_plain:
    lda RR_COLOR
.Lrr_fill:
    dey
    sta (__rc10),y
    ; STA preserves the DEY flags, including widths128..160.
    bne .Lrr_fill
.Lrr_advance:
    clc
    ldy #4
    lda (__rc4),y
    ldy #0
    adc (__rc4),y
    sta (__rc4),y
    ldy #5
    lda (__rc4),y
    ldy #1
    adc (__rc4),y
    sta (__rc4),y
    ldy #6
    lda (__rc4),y
    ldy #2
    adc (__rc4),y
    sta (__rc4),y
    ldy #7
    lda (__rc4),y
    ldy #3
    adc (__rc4),y
    sta (__rc4),y
    clc
    ldy #12
    lda (__rc4),y
    ldy #8
    adc (__rc4),y
    sta (__rc4),y
    ldy #13
    lda (__rc4),y
    ldy #9
    adc (__rc4),y
    sta (__rc4),y
    ldy #14
    lda (__rc4),y
    ldy #10
    adc (__rc4),y
    sta (__rc4),y
    ldy #15
    lda (__rc4),y
    ldy #11
    adc (__rc4),y
    sta (__rc4),y
    inc RR_ROW
    lda RR_ROW
    cmp RR_END
    bne .Lrr_more
    rts
.Lrr_more:
    clc
    lda __rc8
    adc #160
    sta __rc8
    bcc .Lrr_no_carry
    inc __rc9
.Lrr_no_carry:
    jmp .Lrr_row
.Lrr_clamp:
    clc
    lda (__rc4),y
    adc #255
    iny
    lda (__rc4),y
    adc #127
    iny
    lda (__rc4),y
    adc #0
    sta RR_BOUND
    iny
    lda (__rc4),y
    adc #0
    bmi .Lrr_zero
    bne .Lrr_width
    lda RR_BOUND
    cmp #160
    bcs .Lrr_width
    rts
.Lrr_zero:
    lda #0
    rts
.Lrr_width:
    lda #160
    rts

; Conservative static-background bounds. C ABI commands rc2/3, count A/X;
; result low word A/X and high word rc2/3. Renderer checks clear/state/drop.
; Count can be256; byte loop counters deliberately wrap only in that case.
.set SK_COUNT, $77a0
.set SK_LOOP, $77a1
.set SK_STATIC, $77a2
.set SK_MASK, $77a4
.set SK_MIN, $77a8
.set SK_MAX, $77a9
.set SK_END, $77aa
.set SK_BIT, $77ab
.set SK_TEMP, $77ac
.set SK_BAND, $77ad
.section .tables.static_sky,"ax",@progbits
.globl fast_static_sky_bands
fast_static_sky_bands:
    sta SK_COUNT
    cpx #0
    bne .Lsky_begin
    cmp #0
    bne .Lsky_begin
    jmp .Lsky_none
.Lsky_begin:
    lda __rc2
    sta __rc4
    lda __rc3
    sta __rc5
    lda #0
    sta SK_LOOP
    sta SK_STATIC
    lda #255
    sta SK_MASK
    sta SK_MASK+1
    sta SK_MASK+2
    lda #1
    sta SK_MASK+3
.Lsky_triangle:
    ldy #15
    lda (__rc4),y
    and #2
    beq .Lsky_bounds
    inc SK_STATIC
    jmp .Lsky_next
.Lsky_bounds:
    lda #200
    sta SK_MIN
    lda #0
    sta SK_MAX
    ldy #6
.Lsky_vertex:
    jsr .Lsky_clamp_y
    cmp SK_MIN
    bcs .Lsky_minimum
    sta SK_MIN
.Lsky_minimum:
    cmp SK_MAX
    bcc .Lsky_maximum
    sta SK_MAX
.Lsky_maximum:
    iny
    cpy #12
    bne .Lsky_vertex
    lda SK_MIN
    cmp SK_MAX
    bcs .Lsky_next
    lsr
    lsr
    lsr
    sta SK_BAND
    lda SK_MAX
    clc
    adc #7
    lsr
    lsr
    lsr
    sta SK_END
.Lsky_clear_band:
    lda SK_BAND
    and #7
    tax
    lda .Lsky_bits,x
    sta SK_BIT
    lda SK_BAND
    lsr
    lsr
    lsr
    tax
    lda SK_MASK,x
    and SK_BIT
    sta SK_MASK,x
    inc SK_BAND
    lda SK_BAND
    cmp SK_END
    bcc .Lsky_clear_band
.Lsky_next:
    clc
    lda __rc4
    adc #16
    sta __rc4
    bcc .Lsky_pointer
    inc __rc5
.Lsky_pointer:
    inc SK_LOOP
    lda SK_LOOP
    cmp SK_COUNT
    beq .Lsky_done
    jmp .Lsky_triangle
.Lsky_done:
    lda SK_STATIC
    cmp #14
    bne .Lsky_none
    lda SK_MASK+2
    sta __rc2
    lda SK_MASK+3
    sta __rc3
    lda SK_MASK
    ldx SK_MASK+1
    rts
.Lsky_none:
    lda #0
    tax
    sta __rc2
    sta __rc3
    rts
.Lsky_clamp_y:
    lda (__rc4),y
    sta SK_TEMP
    iny
    lda (__rc4),y
    bmi .Lsky_zero
    bne .Lsky_height
    lda SK_TEMP
    cmp #200
    bcs .Lsky_height
    rts
.Lsky_zero:
    lda #0
    rts
.Lsky_height:
    lda #200
    rts
.Lsky_bits:
    .byte $fe,$fd,$fb,$f7,$ef,$df,$bf,$7f
