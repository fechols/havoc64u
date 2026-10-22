; Stable descending signed-depth order of the renderer's commands.
; void fast_depth_sort(const Triangle *commands, uint16_t count, uint16_t segments)
; C ABI: commands rc2/3, count A (low byte) and X (high byte; count 256 has
; X = 1), segments rc4 (prefix) and rc5 (pivot, modulo 256). The commands
; stay in emission order; ORDER[i] receives the index of the command at
; sorted position i for i in 0..count, and PIXELS prepares the raster heads
; through it. Ties keep the logical order: with a nonzero prefix the commands
; [prefix, pivot) come first (the foreground), then [0, prefix) (the scenery
; staged before it), then [pivot, count); otherwise emission order. The key
; of a command is !(depth ^ $8000), so a stable least-significant-digit radix
; sort with ascending buckets yields descending signed depth: one pass over
; the low key byte, one over the high key byte. Each pass counts its digit,
; turns the counts into inclusive prefix sums (the end of every bucket) and
; scatters the index list from the last command down, which keeps equal keys
; in order. Both key bytes are extracted once (KEYLO/KEYHI, indexed by
; command, with the low-byte counting fused in), the logical order is laid
; out in the ORDER page itself (dead until the second pass writes the
; result), the intermediate index list lives in IDXB and the 256 bucket
; counts in COUNTS; all of it is the pixel-band workspace at $8E00-$91FF,
; dead between the last projection and the first raster band. The high key
; byte spans few values in practice (depths below 4096), so its pass zeroes
; and prefixes only the range it saw.
; rc2..rc9 are caller-saved; no static frame or software stack is needed.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7
.zeropage __rc8
.zeropage __rc9
.set KEYLO, $8e00
.set KEYHI, $8f00
.set IDXB, $9000
.set COUNTS, $9100
; The mos-sim harness relocates the output page.
.ifndef ORDER
.set ORDER, $7600
.endif
; rc8: count (0 means 256); rc6/rc7: lowest/highest KEYHI seen; rc9: temporary

.section .tables.sort,"ax",@progbits
.globl fast_depth_sort
fast_depth_sort:
    cpx #0
    bne .Lrs_begin
    cmp #2
    bcs .Lrs_begin
    ; one command keeps position 0; none needs nothing
    tax
    beq .Lrs_nothing
    lda #0
    sta ORDER
.Lrs_nothing:
    rts
.Lrs_begin:
    sta __rc8
    ; low-byte bucket counts start at zero
    lda #0
    tax
.Lrs_zero1:
    sta COUNTS,x
    sta COUNTS+32,x
    sta COUNTS+64,x
    sta COUNTS+96,x
    sta COUNTS+128,x
    sta COUNTS+160,x
    sta COUNTS+192,x
    sta COUNTS+224,x
    inx
    cpx #32
    bne .Lrs_zero1
    lda #$ff
    sta __rc6
    lda #0
    sta __rc7
    ; key bytes in emission order, low-byte counts, high-key range
    ldx #0
.Lrs_keys:
    ldy #12
    lda (__rc2),y
    eor #$ff
    sta KEYLO,x
    stx __rc9
    tax
    inc COUNTS,x
    ldx __rc9
    iny
    lda (__rc2),y
    eor #$7f
    sta KEYHI,x
    cmp __rc6
    bcs .Lrs_not_lower
    sta __rc6
.Lrs_not_lower:
    cmp __rc7
    bcc .Lrs_not_higher
    sta __rc7
.Lrs_not_higher:
    clc
    lda __rc2
    adc #16
    sta __rc2
    bcc .Lrs_keys_next
    inc __rc3
.Lrs_keys_next:
    inx
    cpx __rc8
    bne .Lrs_keys

    ; ---- logical order into ORDER: [prefix, pivot), [0, prefix), [pivot, count) ----
    ldy #0
    lda __rc4
    beq .Lrs_identity
    tax
.Lrs_segment1:
    txa
    sta ORDER,y
    iny
    inx
    cpx __rc5
    bne .Lrs_segment1
    ldx #0
.Lrs_segment2:
    txa
    sta ORDER,y
    iny
    inx
    cpx __rc4
    bne .Lrs_segment2
    ldx __rc5
.Lrs_segment3:
    cpx __rc8
    beq .Lrs_ordered
    txa
    sta ORDER,y
    iny
    inx
    jmp .Lrs_segment3
.Lrs_identity:
    tya
    sta ORDER,y
    iny
    cpy __rc8
    bne .Lrs_identity
.Lrs_ordered:

    ; ---- pass 1: low key byte, logical order -> IDXB ----
    ; COUNTS[d] becomes the end of bucket d (256 wraps to 0; dec restores 255)
    lda #0
    tax
.Lrs_prefix1:
    clc
    adc COUNTS,x
    sta COUNTS,x
    inx
    bne .Lrs_prefix1
    ; scatter from the last command down keeps equal keys in logical order
    ldy __rc8
    dey
.Lrs_scatter1:
    ldx ORDER,y
    lda KEYLO,x
    tax
    dec COUNTS,x
    lda COUNTS,x
    tax
    lda ORDER,y
    sta IDXB,x
    dey
    cpy #$ff
    bne .Lrs_scatter1

    ; ---- pass 2: high key byte over its seen range, IDXB -> ORDER ----
    lda #0
    ldx __rc6
.Lrs_zero2:
    sta COUNTS,x
    cpx __rc7
    beq .Lrs_zeroed2
    inx
    bne .Lrs_zero2
.Lrs_zeroed2:
    ldy __rc8
    dey
.Lrs_count2:
    ldx IDXB,y
    lda KEYHI,x
    tax
    inc COUNTS,x
    dey
    cpy #$ff
    bne .Lrs_count2
    lda #0
    ldx __rc6
.Lrs_prefix2:
    clc
    adc COUNTS,x
    sta COUNTS,x
    cpx __rc7
    beq .Lrs_prefixed2
    inx
    bne .Lrs_prefix2
.Lrs_prefixed2:
    ldy __rc8
    dey
.Lrs_scatter2:
    ldx IDXB,y
    lda KEYHI,x
    tax
    dec COUNTS,x
    lda COUNTS,x
    tax
    lda IDXB,y
    sta ORDER,x
    dey
    cpy #$ff
    bne .Lrs_scatter2
    rts
