; Inclusive signed swept-depth intersection, with wrapping bound arithmetic.
; C ABI: from A/X, to rc2/3, target rc4/5, radius rc6/7; result A (0 or 1).
; Only caller-saved rc2..11 and A/X/Y are touched. No scratch RAM or stack frame.
; V correction after high-byte SBC implements signed 16-bit comparisons even
; across the -32768/32767 boundary; target +/- radius wraps like release Rust.
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

.section .phase.sim_aux.fast_swept_z,"ax",@progbits
.globl fast_swept_z
.type fast_swept_z,@function
fast_swept_z:
    sta __rc8
    stx __rc9
    sec
    sbc __rc2
    txa
    sbc __rc3
    bvc .Lsw_order_sign
    eor #$80
.Lsw_order_sign:
    bmi .Lsw_ordered
    ; rc8/9 becomes min(from,to), rc2/3 becomes max(from,to).
    lda __rc8
    ldx __rc2
    stx __rc8
    sta __rc2
    lda __rc9
    ldx __rc3
    stx __rc9
    sta __rc3
.Lsw_ordered:
    ; Reject when the wrapped upper bound is below the smaller endpoint.
    clc
    lda __rc4
    adc __rc6
    sta __rc10
    lda __rc5
    adc __rc7
    tax
    sec
    lda __rc10
    sbc __rc8
    txa
    sbc __rc9
    bvc .Lsw_upper_sign
    eor #$80
.Lsw_upper_sign:
    bmi .Lsw_false
    ; Reject when the larger endpoint is below the wrapped lower bound.
    sec
    lda __rc4
    sbc __rc6
    sta __rc10
    lda __rc5
    sbc __rc7
    sta __rc11
    sec
    lda __rc2
    sbc __rc10
    lda __rc3
    sbc __rc11
    bvc .Lsw_lower_sign
    eor #$80
.Lsw_lower_sign:
    bmi .Lsw_false
    lda #1
    rts
.Lsw_false:
    lda #0
    rts
.size fast_swept_z, .-fast_swept_z
