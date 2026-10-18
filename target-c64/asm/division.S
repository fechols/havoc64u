; Exact bounded unsigned 32/16 -> 16 division, shared by GEOMETRY and PIXELS.
; Preconditions: denominator != 0 and numerator / denominator < 65536.
; MOS C ABI numerator: A,X,rc2,rc3; denominator: rc4,rc5; result: A,X.
; Only volatile rc2..7 are touched. No static scratch or software stack used.
.section .text.fast_bounded_division,"ax",@progbits
.globl fast_bounded_division
.type fast_bounded_division,@function
fast_bounded_division:
    sta __rc6
    stx __rc7
    ldy #16
.Lbd_bit:
    asl __rc6
    rol __rc7
    rol __rc2
    rol __rc3
    bcs .Lbd_overflow
    sec
    lda __rc2
    sbc __rc4
    tax
    lda __rc3
    sbc __rc5
    bcc .Lbd_next
.Lbd_subtracted:
    sta __rc3
    stx __rc2
    inc __rc6
.Lbd_next:
    dey
    bne .Lbd_bit
    lda __rc6
    ldx __rc7
    rts
.Lbd_overflow:
    ; The seventeenth remainder bit proves subtraction is necessary. Carry
    ; remains set from ROL, and the low sixteen result bits are exact.
    lda __rc2
    sbc __rc4
    tax
    lda __rc3
    sbc __rc5
    jmp .Lbd_subtracted
.size fast_bounded_division, .-fast_bounded_division

; Exact signed Q16 edge slope: (dx * 65536) / positive dy, toward zero.
; ABI dx A/X, dy rc2/3; return A/X/rc2/3. Only volatile rc2..10 used.
; A 16-bit divisor and known zero low half avoid general signed32/32 division.
.section .phase.pixels.edge_step,"ax",@progbits
.globl fast_edge_step
.type fast_edge_step,@function
fast_edge_step:
    sta __rc6
    stx __rc7
    stx __rc10
    ora __rc7
    bne .Les_nonzero
    tax
    sta __rc2
    sta __rc3
    rts
.Les_nonzero:
    txa
    bpl .Les_positive
    sec
    lda #0
    sbc __rc6
    sta __rc6
    lda #0
    sbc __rc7
    sta __rc7
.Les_positive:
    lda #0
    sta __rc4
    sta __rc5
    sta __rc8
    sta __rc9
    ldy #32
.Les_leading:
    lda __rc7
    bmi .Les_bit
    asl __rc4
    rol __rc5
    rol __rc6
    rol __rc7
    dey
    bne .Les_leading
.Les_bit:
    asl __rc4
    rol __rc5
    rol __rc6
    rol __rc7
    rol __rc8
    rol __rc9
    bcs .Les_overflow
    sec
    lda __rc8
    sbc __rc2
    tax
    lda __rc9
    sbc __rc3
    bcc .Les_next
.Les_subtracted:
    sta __rc9
    stx __rc8
    inc __rc4
.Les_next:
    dey
    bne .Les_bit
    lda __rc10
    bpl .Les_return
    sec
    lda #0
    sbc __rc4
    sta __rc4
    lda #0
    sbc __rc5
    sta __rc5
    lda #0
    sbc __rc6
    sta __rc6
    lda #0
    sbc __rc7
    sta __rc7
.Les_return:
    lda __rc6
    sta __rc2
    lda __rc7
    sta __rc3
    lda __rc4
    ldx __rc5
    rts
.Les_overflow:
    lda __rc8
    sbc __rc2
    tax
    lda __rc9
    sbc __rc3
    jmp .Les_subtracted
.size fast_edge_step, .-fast_edge_step
