; Stable descending signed-depth radix sort. C ABI: commands rc2/3, count A/X.
; Four 4-bit index passes, then one permutation of the 16-byte commands.
; Scratch $8C00-$8E5A belongs to the retired projection cache. No IRQ access.
; Counts/positions are modulo 256: even a full 256-entry bucket is valid.
; rc2..11 are caller-saved; no compiler static frame or software stack needed.
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
.set RS_COUNTS, $8e00
.set RS_POSITIONS, $8e10
.set RS_SAVED, $8e40
.set RS_COUNT, $8e50
.set RS_PASS, $8e51
.set RS_LOOP, $8e52
.set RS_INDEX, $8e53
.set RS_XOR, $8e54
.set RS_START, $8e56
.set RS_DEST, $8e57
.set RS_SOURCE, $8e58
.set RS_TEMP, $8e59

.section .tables.sort,"ax",@progbits
.globl fast_depth_sort
fast_depth_sort:
    sta RS_COUNT
    cpx #0
    bne .Lrs_begin
    cmp #2
    bcs .Lrs_begin
    rts
.Lrs_begin:
    ldx #0
.Lrs_identity:
    txa
    sta $8c00,x
    inx
    cpx RS_COUNT
    bne .Lrs_identity
    lda #0
    sta __rc4
    sta __rc6
    sta RS_PASS
    lda #$8c
    sta __rc5
    lda #$8d
    sta __rc7
.Lrs_pass:
    lda #15
    ldx RS_PASS
    cpx #3
    bne .Lrs_xor_ready
    lda #7
.Lrs_xor_ready:
    sta RS_XOR
    ldx #15
    lda #0
.Lrs_zero_counts:
    sta RS_COUNTS,x
    dex
    bpl .Lrs_zero_counts
    sta RS_LOOP
.Lrs_count:
    ldy RS_LOOP
    lda (__rc4),y
    jsr .Lrs_digit
    tax
    inc RS_COUNTS,x
    inc RS_LOOP
    lda RS_LOOP
    cmp RS_COUNT
    bne .Lrs_count
    ldx #0
    lda #0
.Lrs_prefix:
    sta RS_POSITIONS,x
    clc
    adc RS_COUNTS,x
    inx
    cpx #16
    bne .Lrs_prefix
    lda #0
    sta RS_LOOP
.Lrs_scatter:
    ldy RS_LOOP
    lda (__rc4),y
    sta RS_INDEX
    jsr .Lrs_digit
    tax
    ldy RS_POSITIONS,x
    inc RS_POSITIONS,x
    lda RS_INDEX
    sta (__rc6),y
    inc RS_LOOP
    lda RS_LOOP
    cmp RS_COUNT
    bne .Lrs_scatter
    lda __rc5
    ldx __rc7
    sta __rc7
    stx __rc5
    inc RS_PASS
    lda RS_PASS
    cmp #4
    bne .Lrs_pass
    ; The second index page is dead; reuse it as 256 visited bytes.
    ldx #0
    txa
.Lrs_zero_visited:
    sta $8d00,x
    inx
    bne .Lrs_zero_visited
    stx RS_START
.Lrs_cycle_start:
    ldx RS_START
    lda $8d00,x
    bne .Lrs_next_cycle
    txa
    sta RS_DEST
    jsr .Lrs_pointer
    ldy #15
.Lrs_save:
    lda (__rc8),y
    sta RS_SAVED,y
    dey
    bpl .Lrs_save
.Lrs_cycle:
    ldx RS_DEST
    lda #1
    sta $8d00,x
    lda $8c00,x
    sta RS_SOURCE
    txa
    jsr .Lrs_pointer
    lda __rc8
    sta __rc10
    lda __rc9
    sta __rc11
    lda RS_SOURCE
    cmp RS_START
    beq .Lrs_restore
    jsr .Lrs_pointer
    ldy #15
.Lrs_move:
    lda (__rc8),y
    sta (__rc10),y
    dey
    bpl .Lrs_move
    lda RS_SOURCE
    sta RS_DEST
    jmp .Lrs_cycle
.Lrs_restore:
    ldy #15
.Lrs_restore_byte:
    lda RS_SAVED,y
    sta (__rc10),y
    dey
    bpl .Lrs_restore_byte
.Lrs_next_cycle:
    inc RS_START
    lda RS_START
    cmp RS_COUNT
    bne .Lrs_cycle_start
    rts

; A=command index -> rc8/9=command pointer. Preserves base, index-page pointers.
.Lrs_pointer:
    sta RS_TEMP
    asl
    asl
    asl
    asl
    clc
    adc __rc2
    sta __rc8
    lda #0
    adc __rc3
    sta __rc9
    lda RS_TEMP
    lsr
    lsr
    lsr
    lsr
    clc
    adc __rc9
    sta __rc9
    rts

; A=command index -> A=transformed depth nibble for this pass.
.Lrs_digit:
    jsr .Lrs_pointer
    ldy #12
    lda RS_PASS
    and #2
    beq .Lrs_depth_byte
    iny
.Lrs_depth_byte:
    lda (__rc8),y
    ldx RS_PASS
    txa
    and #1
    beq .Lrs_low_nibble
    lda (__rc8),y
    lsr
    lsr
    lsr
    lsr
    jmp .Lrs_nibble
.Lrs_low_nibble:
    lda (__rc8),y
.Lrs_nibble:
    and #15
    eor RS_XOR
    rts
