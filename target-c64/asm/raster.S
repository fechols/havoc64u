; Exact flat-triangle row walker. C ABI: job pointer rc2/3, pixels rc4/5.
; Job layout: three 12-byte edges {i16 y0,y1; i32 x,step}, followed by
; u8 first_y,end_y,band_y,color. Pixel rows are always within 0..199.
; rc6..13 and $77A0..$77B5 are volatile main-thread scratch. The pack kernel
; reuses this scratch later; audio IRQ never accesses it. rc20..31 are kept.
.set RR_ROW, $77a0
.set RR_END, $77a1
.set RR_COLOR, $77a2
.set RR_LEFT, $77a4
.set RR_RIGHT, $77a8
.set RR_VALUE, $77ac
.set RR_EDGES, $77b0
.set RR_FIRST, $77b1
.set RR_LAST, $77b2

.section .phase.pixels,"ax",@progbits
.globl fast_raster_rows
fast_raster_rows:
    lda __rc2
    sta __rc6
    lda __rc3
    sta __rc7
    lda __rc4
    sta __rc8
    lda __rc5
    sta __rc9
    ldy #36
    lda (__rc6),y
    sta RR_ROW
    iny
    cmp (__rc6),y
    bne .Lrr_nonempty
    rts
.Lrr_nonempty:
    lda (__rc6),y
    sta RR_END
    iny
    lda RR_ROW
    sec
    sbc (__rc6),y
    tax
    iny
    lda (__rc6),y
    sta RR_COLOR
    cpx #0
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
    lda #$ff
    sta RR_LEFT
    sta RR_LEFT+1
    sta RR_LEFT+2
    lda #$7f
    sta RR_LEFT+3
    lda #0
    sta RR_RIGHT
    sta RR_RIGHT+1
    sta RR_RIGHT+2
    lda #$80
    sta RR_RIGHT+3
    lda __rc6
    sta __rc10
    lda __rc7
    sta __rc11
    lda #3
    sta RR_EDGES
.Lrr_edge:
    ; Active iff y >= y0 and y < y1. y is an unsigned visible scanline;
    ; negative/large signed endpoints can therefore be rejected cheaply.
    ldy #1
    lda (__rc10),y
    bmi .Lrr_check_end
    beq .Lrr_check_start_low
    jmp .Lrr_next_edge
.Lrr_check_start_low:
    dey
    lda RR_ROW
    cmp (__rc10),y
    bcs .Lrr_check_end
    jmp .Lrr_next_edge
.Lrr_check_end:
    ldy #3
    lda (__rc10),y
    bpl .Lrr_check_end_nonnegative
    jmp .Lrr_next_edge
.Lrr_check_end_nonnegative:
    bne .Lrr_active
    dey
    lda RR_ROW
    cmp (__rc10),y
    bcc .Lrr_active
    jmp .Lrr_next_edge
.Lrr_active:
    ldy #4
    ldx #0
.Lrr_read_x:
    lda (__rc10),y
    sta RR_VALUE,x
    iny
    inx
    cpx #4
    bne .Lrr_read_x
    ; Signed 32-bit minimum, using a biased most-significant byte.
    lda RR_VALUE+3
    eor #$80
    sta RR_FIRST
    lda RR_LEFT+3
    eor #$80
    cmp RR_FIRST
    bcc .Lrr_maximum
    bne .Lrr_set_minimum
    ldx #2
.Lrr_compare_minimum:
    lda RR_LEFT,x
    cmp RR_VALUE,x
    bcc .Lrr_maximum
    bne .Lrr_set_minimum
    dex
    bpl .Lrr_compare_minimum
    jmp .Lrr_maximum
.Lrr_set_minimum:
    ldx #3
.Lrr_copy_minimum:
    lda RR_VALUE,x
    sta RR_LEFT,x
    dex
    bpl .Lrr_copy_minimum
.Lrr_maximum:
    lda RR_RIGHT+3
    eor #$80
    cmp RR_FIRST
    bcc .Lrr_set_maximum
    bne .Lrr_advance_edge
    ldx #2
.Lrr_compare_maximum:
    lda RR_RIGHT,x
    cmp RR_VALUE,x
    bcc .Lrr_set_maximum
    bne .Lrr_advance_edge
    dex
    bpl .Lrr_compare_maximum
    jmp .Lrr_advance_edge
.Lrr_set_maximum:
    ldx #3
.Lrr_copy_maximum:
    lda RR_VALUE,x
    sta RR_RIGHT,x
    dex
    bpl .Lrr_copy_maximum
.Lrr_advance_edge:
    clc
    ldy #8
    lda (__rc10),y
    adc RR_VALUE
    ldy #4
    sta (__rc10),y
    ldy #9
    lda (__rc10),y
    adc RR_VALUE+1
    ldy #5
    sta (__rc10),y
    ldy #10
    lda (__rc10),y
    adc RR_VALUE+2
    ldy #6
    sta (__rc10),y
    ldy #11
    lda (__rc10),y
    adc RR_VALUE+3
    ldy #7
    sta (__rc10),y
.Lrr_next_edge:
    clc
    lda __rc10
    adc #12
    sta __rc10
    bcc .Lrr_edge_no_carry
    inc __rc11
.Lrr_edge_no_carry:
    dec RR_EDGES
    beq .Lrr_span
    jmp .Lrr_edge
.Lrr_span:
    ; Clamping is monotonic, so empty/reversed bounds remain empty after
    ; rounding ((x + 127) >> 8) and clamping to [0,160].
    ldx #0
    jsr .Lrr_clamp_bound
    sta RR_FIRST
    ldx #4
    jsr .Lrr_clamp_bound
    sta RR_LAST
    sec
    sbc RR_FIRST
    beq .Lrr_next_row
    bcc .Lrr_next_row
    tay
    clc
    lda __rc8
    adc RR_FIRST
    sta __rc12
    lda __rc9
    adc #0
    sta __rc13
    lda RR_COLOR
.Lrr_fill:
    dey
    sta (__rc12),y
    cpy #0
    bne .Lrr_fill
.Lrr_next_row:
    inc RR_ROW
    lda RR_ROW
    cmp RR_END
    bne .Lrr_more_rows
    rts
.Lrr_more_rows:
    clc
    lda __rc8
    adc #160
    sta __rc8
    bcc .Lrr_row_no_carry
    inc __rc9
.Lrr_row_no_carry:
    jmp .Lrr_row
.Lrr_clamp_bound:
    lda RR_LEFT+3,x
    bmi .Lrr_clamp_zero
    ora RR_LEFT+2,x
    bne .Lrr_clamp_width
    clc
    lda RR_LEFT,x
    adc #127
    lda RR_LEFT+1,x
    adc #0
    bcs .Lrr_clamp_width
    cmp #160
    bcs .Lrr_clamp_width
    rts
.Lrr_clamp_width:
    lda #160
    rts
.Lrr_clamp_zero:
    lda #0
    rts
