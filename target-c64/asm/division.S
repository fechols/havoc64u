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
