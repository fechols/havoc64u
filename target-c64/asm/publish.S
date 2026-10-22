; Publish one finished eight-line band without changing memory being fetched
; by the VIC. C ABI: band 0..24 in A; I/O mapped in, IRQs enabled on entry.
; Staging: bitmap $9540..$967f, screen $7700..$7727, color $7728..$774f.
; The staging bytes and rc0/1, rc20..31 are preserved; rc2..15 are scratch.
;
; For every Y-scroll 0..7, band b can be fetched on lines 48+8*b through
; 62+8*b. Exclude [40+8*b,64+8*b): eight leading lines cover the bounded
; copy, with a trailing line of margin. The last band's upper bound is 256.
; Poll with IRQs enabled, then mask and resample before the actual stores.
; Only this final sample and fixed copy delay audio/the line-251 effect IRQ.
; PHP/PLP preserve the caller's flags, including I and D; arithmetic uses CLD.
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

.section .phase.pixels.publish,"ax",@progbits
.globl publish_staged_band
.type publish_staged_band,@function
publish_staged_band:
    php
    cld
    ; rc14/15 = 40*band, first as 8*band then plus 32*band.
    asl
    asl
    asl
    sta __rc14
    clc
    adc #40
    sta __rc8
    adc #24
    sta __rc9
    lda #0
    adc #0
    sta __rc10
    lda #0
    sta __rc15
    lda __rc14
    asl
    rol __rc15
    asl
    rol __rc15
    clc
    adc __rc14
    sta __rc14
    lda __rc15
    adc #0
    sta __rc15
    ; rc4/5 = screen, rc6/7 = color, rc2/3 = bitmap.
    lda __rc14
    sta __rc4
    sta __rc6
    lda __rc15
    clc
    adc #$80
    sta __rc5
    clc
    adc #$58
    sta __rc7
    lda __rc14
    asl
    rol __rc15
    asl
    rol __rc15
    asl
    rol __rc15
    sta __rc2
    lda __rc15
    clc
    adc #$a0
    sta __rc3

.Lpublish_retry:
    cli
    lda #9
    sta PROF_DETAIL
.globl publish_wait
publish_wait:
.Lpublish_wait:
    jsr .Lpublish_safe
    bcc .Lpublish_wait
    lda #4
    sta PROF_DETAIL
    sei
.globl publish_guard
publish_guard:
    jsr .Lpublish_safe
    bcc .Lpublish_retry
.globl publish_commit
publish_commit:
    ; 256 + 64 bitmap bytes; then both 40-byte attribute tables.
    ldy #0
.Lpublish_page:
    .rept 8
    lda $9540,y
    sta (__rc2),y
    iny
    .endr
    bne .Lpublish_page
    inc __rc3
.Lpublish_tail:
    .rept 8
    lda $9640,y
    sta (__rc2),y
    iny
    .endr
    cpy #64
    bne .Lpublish_tail
    ldy #0
.Lpublish_attributes:
    .rept 4
    lda $7700,y
    sta (__rc4),y
    lda $7728,y
    sta (__rc6),y
    iny
    .endr
    cpy #40
    bne .Lpublish_attributes
.globl publish_done
publish_done:
    plp
    rts

; Return carry set when the complete commit can finish outside the band's
; fetch lifetime. Read high/low/high so crossing raster 255 or frame wrap
; cannot fabricate a line. IRQs may run while this is used by the wait loop;
; the masked second call always validates the current position afresh.
.Lpublish_safe:
    lda $d011
    and #$80
    sta __rc12
    lda $d012
    sta __rc13
    lda $d011
    and #$80
    cmp __rc12
    bne .Lpublish_safe
    lda __rc12
    bne .Lpublish_yes
    lda __rc13
    cmp __rc8
    bcc .Lpublish_yes
    lda __rc10
    bne .Lpublish_no
    lda __rc13
    cmp __rc9
    bcs .Lpublish_yes
.Lpublish_no:
    clc
    rts
.Lpublish_yes:
    sec
    rts
.size publish_staged_band, .-publish_staged_band
