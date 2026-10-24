; DLT4 block decoder. MOS C ABI: frame count in A, Kernel in rc2/3,
; source in rc4/5, output in rc6/7; return compressed bytes consumed in A.
; rc16..19 cache the signed16 predictors and are saved/restored on the stack.
; Aligned groups consume three bytes and publish eight samples (four stereo
; frames or eight mono frames). Generic prefixes/tails preserve every reservoir
; boundary, including callers entering with an odd number of valid stereo bits.
; Kernel: signed16 books[16] at0, predictors at32/34, reservoir36/37,
; valid bits38, channels39. Saturation precedes rounded signed8 publication.
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

; Y is the byte offset of the selected signed16 delta. Return rounded PCM in A.
; Names give each expansion private overflow labels without helper-call costs.
.macro DIGITAL_SAMPLE name, low, high
    clc
    lda (__rc8),y
    adc \low
    sta \low
    iny
    lda (__rc8),y
    adc \high
    bvc .Ldigital_sum_\name
    bmi .Ldigital_positive_\name
    lda #0
    sta \low
    lda #128
    bne .Ldigital_sum_\name
.Ldigital_positive_\name:
    lda #255
    sta \low
    lda #127
.Ldigital_sum_\name:
    sta \high
    lda \low
    cmp #128
    lda \high
    adc #0
    bvc .Ldigital_rounded_\name
    lda #127
.Ldigital_rounded_\name:
.endm

.section .phase.audio.kernel,"ax",@progbits
.globl digital_decode_run
.type digital_decode_run,@function
digital_decode_run:
    ; Remap arguments before reusing the output pair for frame/input counts.
    sta __rc10
    lda __rc2
    sta __rc8
    lda __rc3
    sta __rc9
    lda __rc4
    sta __rc2
    lda __rc5
    sta __rc3
    lda __rc6
    sta __rc4
    lda __rc7
    sta __rc5
    lda __rc10
    sta __rc6
    lda __rc16
    pha
    lda __rc17
    pha
    lda __rc18
    pha
    lda __rc19
    pha
    ldy #32
    lda (__rc8),y
    sta __rc16
    iny
    lda (__rc8),y
    sta __rc17
    iny
    lda (__rc8),y
    sta __rc18
    iny
    lda (__rc8),y
    sta __rc19
    iny
    lda (__rc8),y
    sta __rc10
    iny
    lda (__rc8),y
    sta __rc11
    iny
    lda (__rc8),y
    sta __rc12
    iny
    lda (__rc8),y
    sta __rc13
    lda #0
    sta __rc7
.Ldigital_dispatch:
    lda __rc6
    bne .Ldigital_remaining
    jmp .Ldigital_done
.Ldigital_remaining:
    lda __rc12
    bne .Ldigital_tail
    lda __rc13
    cmp #2
    beq .Ldigital_stereo_check
    lda __rc6
    cmp #8
    bcc .Ldigital_tail
    jmp .Ldigital_mono_group
.Ldigital_stereo_check:
    lda __rc6
    cmp #4
    bcc .Ldigital_tail
    jmp .Ldigital_stereo_group
.Ldigital_tail:
    jsr .Ldigital_next_code
    DIGITAL_SAMPLE tail_left, __rc16, __rc17
    ldy #0
    sta (__rc4),y
    inc __rc4
    bne .Ldigital_tail_channel
    inc __rc5
.Ldigital_tail_channel:
    lda __rc13
    cmp #2
    bne .Ldigital_tail_frame
    jsr .Ldigital_next_code
    tya
    ora #16
    tay
    DIGITAL_SAMPLE tail_right, __rc18, __rc19
    ldy #0
    sta (__rc4),y
    inc __rc4
    bne .Ldigital_tail_frame
    inc __rc5
.Ldigital_tail_frame:
    dec __rc6
    jmp .Ldigital_dispatch

; The fallback is independent of the aligned path. Return code*2 in Y.
; The reservoir and count in rc10..12 survive until the next frame dispatch.
.Ldigital_next_code:
    lda __rc12
    cmp #3
    bcs .Ldigital_code
    ldy __rc7
    lda (__rc2),y
    inc __rc7
    sta __rc14
    lda #0
    sta __rc15
    ldx __rc12
    beq .Ldigital_refilled
.Ldigital_align:
    asl __rc14
    rol __rc15
    dex
    bne .Ldigital_align
.Ldigital_refilled:
    lda __rc14
    ora __rc10
    sta __rc10
    lda __rc15
    sta __rc11
    lda __rc12
    clc
    adc #8
    sta __rc12
.Ldigital_code:
    lda __rc10
    and #7
    asl
    tay
    lsr __rc11
    ror __rc10
    lsr __rc11
    ror __rc10
    lsr __rc11
    ror __rc10
    lda __rc12
    sec
    sbc #3
    sta __rc12
    rts

.Ldigital_done:
    ldy #32
    lda __rc16
    sta (__rc8),y
    iny
    lda __rc17
    sta (__rc8),y
    iny
    lda __rc18
    sta (__rc8),y
    iny
    lda __rc19
    sta (__rc8),y
    iny
    lda __rc10
    sta (__rc8),y
    iny
    lda __rc11
    sta (__rc8),y
    iny
    lda __rc12
    sta (__rc8),y
    pla
    sta __rc19
    pla
    sta __rc18
    pla
    sta __rc17
    pla
    sta __rc16
    lda __rc7
    rts

; An aligned group has no carry-in bits. Reuse reservoir registers for its three
; packed bytes, then return to the generic dispatcher with an empty reservoir.
.Ldigital_stereo_group:
    ldy __rc7
    lda (__rc2),y
    sta __rc10
    iny
    lda (__rc2),y
    sta __rc11
    iny
    lda (__rc2),y
    sta __rc12
    iny
    sty __rc7
    ; Code 0, packed bit 0.
    lda __rc10
    asl
    and #14
    tay
    DIGITAL_SAMPLE stereo_0, __rc16, __rc17
    ldy #0
    sta (__rc4),y
    ; Code 1, packed bit 3.
    lda __rc10
    lsr
    lsr
    and #14
    ora #16
    tay
    DIGITAL_SAMPLE stereo_1, __rc18, __rc19
    ldy #1
    sta (__rc4),y
    ; Code 2, packed bit 6.
    lda __rc10
    lsr
    lsr
    lsr
    lsr
    lsr
    and #6
    sta __rc14
    lda __rc11
    asl
    asl
    asl
    and #8
    ora __rc14
    tay
    DIGITAL_SAMPLE stereo_2, __rc16, __rc17
    ldy #2
    sta (__rc4),y
    ; Code 3, packed bit 9.
    lda __rc11
    and #14
    ora #16
    tay
    DIGITAL_SAMPLE stereo_3, __rc18, __rc19
    ldy #3
    sta (__rc4),y
    ; Code 4, packed bit 12.
    lda __rc11
    lsr
    lsr
    lsr
    and #14
    tay
    DIGITAL_SAMPLE stereo_4, __rc16, __rc17
    ldy #4
    sta (__rc4),y
    ; Code 5, packed bit 15.
    lda __rc11
    lsr
    lsr
    lsr
    lsr
    lsr
    lsr
    and #2
    sta __rc14
    lda __rc12
    asl
    asl
    and #12
    ora __rc14
    ora #16
    tay
    DIGITAL_SAMPLE stereo_5, __rc18, __rc19
    ldy #5
    sta (__rc4),y
    ; Code 6, packed bit 18.
    lda __rc12
    lsr
    and #14
    tay
    DIGITAL_SAMPLE stereo_6, __rc16, __rc17
    ldy #6
    sta (__rc4),y
    ; Code 7, packed bit 21.
    lda __rc12
    lsr
    lsr
    lsr
    lsr
    and #14
    ora #16
    tay
    DIGITAL_SAMPLE stereo_7, __rc18, __rc19
    ldy #7
    sta (__rc4),y
    clc
    lda __rc4
    adc #8
    sta __rc4
    bcc .Ldigital_stereo_advanced
    inc __rc5
.Ldigital_stereo_advanced:
    sec
    lda __rc6
    sbc #4
    sta __rc6
    cmp #4
    bcc .Ldigital_stereo_tail
    jmp .Ldigital_stereo_group
.Ldigital_stereo_tail:
    lda #0
    sta __rc10
    sta __rc11
    sta __rc12
    jmp .Ldigital_dispatch

; An aligned group has no carry-in bits. Reuse reservoir registers for its three
; packed bytes, then return to the generic dispatcher with an empty reservoir.
.Ldigital_mono_group:
    ldy __rc7
    lda (__rc2),y
    sta __rc10
    iny
    lda (__rc2),y
    sta __rc11
    iny
    lda (__rc2),y
    sta __rc12
    iny
    sty __rc7
    ; Code 0, packed bit 0.
    lda __rc10
    asl
    and #14
    tay
    DIGITAL_SAMPLE mono_0, __rc16, __rc17
    ldy #0
    sta (__rc4),y
    ; Code 1, packed bit 3.
    lda __rc10
    lsr
    lsr
    and #14
    tay
    DIGITAL_SAMPLE mono_1, __rc16, __rc17
    ldy #1
    sta (__rc4),y
    ; Code 2, packed bit 6.
    lda __rc10
    lsr
    lsr
    lsr
    lsr
    lsr
    and #6
    sta __rc14
    lda __rc11
    asl
    asl
    asl
    and #8
    ora __rc14
    tay
    DIGITAL_SAMPLE mono_2, __rc16, __rc17
    ldy #2
    sta (__rc4),y
    ; Code 3, packed bit 9.
    lda __rc11
    and #14
    tay
    DIGITAL_SAMPLE mono_3, __rc16, __rc17
    ldy #3
    sta (__rc4),y
    ; Code 4, packed bit 12.
    lda __rc11
    lsr
    lsr
    lsr
    and #14
    tay
    DIGITAL_SAMPLE mono_4, __rc16, __rc17
    ldy #4
    sta (__rc4),y
    ; Code 5, packed bit 15.
    lda __rc11
    lsr
    lsr
    lsr
    lsr
    lsr
    lsr
    and #2
    sta __rc14
    lda __rc12
    asl
    asl
    and #12
    ora __rc14
    tay
    DIGITAL_SAMPLE mono_5, __rc16, __rc17
    ldy #5
    sta (__rc4),y
    ; Code 6, packed bit 18.
    lda __rc12
    lsr
    and #14
    tay
    DIGITAL_SAMPLE mono_6, __rc16, __rc17
    ldy #6
    sta (__rc4),y
    ; Code 7, packed bit 21.
    lda __rc12
    lsr
    lsr
    lsr
    lsr
    and #14
    tay
    DIGITAL_SAMPLE mono_7, __rc16, __rc17
    ldy #7
    sta (__rc4),y
    clc
    lda __rc4
    adc #8
    sta __rc4
    bcc .Ldigital_mono_advanced
    inc __rc5
.Ldigital_mono_advanced:
    sec
    lda __rc6
    sbc #8
    sta __rc6
    cmp #8
    bcc .Ldigital_mono_tail
    jmp .Ldigital_mono_group
.Ldigital_mono_tail:
    lda #0
    sta __rc10
    sta __rc11
    sta __rc12
    jmp .Ldigital_dispatch
.size digital_decode_run, .-digital_decode_run