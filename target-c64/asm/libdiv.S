; Compact replacements for the libcrt 8- and 16-bit division libcalls that
; compiled Rust uses (__udivhi3, __umodhi3, __divhi3, __udivqi3, __umodqi3),
; linked with --wrap (target-c64/build.rs) so libcrt's divide object, which
; also holds the 32-bit routines, is never pulled in.
; One shift-subtract core per width serves quotient and remainder, saving
; resident space; hot paths use the dedicated division kernels instead.
; C ABI: 16-bit dividend in A/X and divisor in rc2/3; 8-bit dividend in A
; and divisor in X; results in A (and X). Only caller-saved rc2..rc7, A, X
; and Y change. Rust checks division by zero before calling these.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7

.section .text.libdiv,"ax",@progbits
.globl __wrap___udivhi3
.type __wrap___udivhi3,@function
__wrap___udivhi3:
    jsr .Ldivmod16
    lda __rc4
    ldx __rc5
    rts
.size __wrap___udivhi3, .-__wrap___udivhi3

.globl __wrap___umodhi3
.type __wrap___umodhi3,@function
__wrap___umodhi3:
    jsr .Ldivmod16
    lda __rc6
    ldx __rc7
    rts
.size __wrap___umodhi3, .-__wrap___umodhi3

; Signed: the quotient truncates toward zero (Rust `/` on i16).
.globl __wrap___divhi3
.type __wrap___divhi3,@function
__wrap___divhi3:
    sta __rc4
    stx __rc5
    txa
    eor __rc3
    php
    lda __rc5
    bpl .Ldiv_dividend
    jsr .Lnegate4
.Ldiv_dividend:
    lda __rc3
    bpl .Ldiv_divisor
    sec
    lda #0
    sbc __rc2
    sta __rc2
    lda #0
    sbc __rc3
    sta __rc3
.Ldiv_divisor:
    lda __rc4
    ldx __rc5
    jsr .Ldivmod16
    plp
    bpl .Ldiv_done
    jsr .Lnegate4
.Ldiv_done:
    lda __rc4
    ldx __rc5
    rts
.Lnegate4:
    sec
    lda #0
    sbc __rc4
    sta __rc4
    lda #0
    sbc __rc5
    sta __rc5
    rts
.size __wrap___divhi3, .-__wrap___divhi3

; rc4/5 = A/X / rc2/3, rc6/7 = the remainder (unsigned).
.Ldivmod16:
    sta __rc4
    stx __rc5
    lda #0
    sta __rc6
    sta __rc7
    ldy #16
.Ldivmod16_bit:
    asl __rc4
    rol __rc5
    rol __rc6
    rol __rc7
    bcs .Ldivmod16_take
    lda __rc6
    cmp __rc2
    lda __rc7
    sbc __rc3
    bcc .Ldivmod16_next
.Ldivmod16_take:
    ; Carry is set: the shifted remainder is at least the divisor.
    lda __rc6
    sbc __rc2
    sta __rc6
    lda __rc7
    sbc __rc3
    sta __rc7
    inc __rc4
.Ldivmod16_next:
    dey
    bne .Ldivmod16_bit
    rts

.globl __wrap___udivqi3
.type __wrap___udivqi3,@function
__wrap___udivqi3:
    jsr .Ldivmod8
    lda __rc4
    rts
.size __wrap___udivqi3, .-__wrap___udivqi3

.globl __wrap___umodqi3
.type __wrap___umodqi3,@function
__wrap___umodqi3:
    jmp .Ldivmod8
.size __wrap___umodqi3, .-__wrap___umodqi3

; rc4 = A / X; the remainder in A (unsigned).
.Ldivmod8:
    sta __rc4
    stx __rc2
    lda #0
    ldy #8
.Ldivmod8_bit:
    asl __rc4
    rol
    bcs .Ldivmod8_take
    cmp __rc2
    bcc .Ldivmod8_next
.Ldivmod8_take:
    sbc __rc2
    inc __rc4
.Ldivmod8_next:
    dey
    bne .Ldivmod8_bit
    rts
