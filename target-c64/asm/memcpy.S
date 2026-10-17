; Native LLVM-MOS C memcpy: destination rc2/3, source rc4/5, count A/X.
; Return the original destination in rc2/3. Only caller-clobbered registers
; are used; there is no software-stack frame or compiler static scratch.
; Source and destination must not overlap, as required by memcpy.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7
.zeropage __rc8

.section .text.memcpy,"ax",@progbits
.globl memcpy
.type memcpy,@function
memcpy:
    sta __rc8
    lda __rc2
    sta __rc6
    lda __rc3
    sta __rc7
    ldy #0
    cpx #0
    beq .Lmemcpy_tail
.Lmemcpy_page:
    lda (__rc4),y
    sta (__rc6),y
    iny
    bne .Lmemcpy_page
    inc __rc5
    inc __rc7
    dex
    bne .Lmemcpy_page
.Lmemcpy_tail:
    cpy __rc8
    beq .Lmemcpy_done
    lda (__rc4),y
    sta (__rc6),y
    iny
    jmp .Lmemcpy_tail
.Lmemcpy_done:
    rts
.size memcpy, .-memcpy
