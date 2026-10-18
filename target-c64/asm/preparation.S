; Exact Q16 triangle setup: rc2/3=input16, rc4/5=head16, rc6/7=tail12.
; Input is copied before any output write, including when head aliases input.
; Only caller-save RC2..19 and phase-owned $77A0..$77BE are modified.
; Preparation finishes before row walking and packing reuse this scratch.
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
.set TP_POINTS, $77a0
.set TP_HEAD, $77b0
.set TP_TAIL, $77b2
.set TP_FLAGS, $77b4
.set TP_SWAP, $77b5
.set TP_AX, $77b6
.set TP_AY, $77b8
.set TP_DX, $77ba
.set TP_DY, $77bc
.set TP_SIGN, $77be

.section .phase.pixels.preparation,"ax",@progbits
.globl fast_prepare_triangle
.type fast_prepare_triangle,@function
fast_prepare_triangle:
    lda __rc4
    sta TP_HEAD
    lda __rc5
    sta TP_HEAD+1
    lda __rc6
    sta TP_TAIL
    lda __rc7
    sta TP_TAIL+1
    ldy #15
.Lpt_copy:
    lda (__rc2),y
    sta TP_POINTS,y
    dey
    bpl .Lpt_copy
    lda TP_POINTS+15
    and #4
    asl
    asl
    ora TP_POINTS+14
    sta TP_FLAGS
    lda TP_POINTS+15
    and #$60
    ora TP_FLAGS
    sta TP_FLAGS
    ldx #0
    ldy #2
    jsr .Lpt_sort_pair
    ldx #2
    ldy #4
    jsr .Lpt_sort_pair
    ldx #0
    ldy #2
    jsr .Lpt_sort_pair
    lda TP_TAIL
    sta __rc18
    lda TP_TAIL+1
    sta __rc19
    lda TP_POINTS+6
    ldx TP_POINTS+7
    jsr .Lpt_clamp
    ldy #8
    sta (__rc18),y
    lda TP_POINTS+8
    ldx TP_POINTS+9
    jsr .Lpt_clamp
    ldy #9
    sta (__rc18),y
    lda TP_POINTS+10
    ldx TP_POINTS+11
    jsr .Lpt_clamp
    ldy #10
    sta (__rc18),y
    lda TP_FLAGS
    iny
    sta (__rc18),y
    lda TP_HEAD
    sta __rc18
    lda TP_HEAD+1
    sta __rc19
    ldx #0
    ldy #4
    jsr .Lpt_edge
    clc
    lda TP_HEAD
    adc #8
    sta __rc18
    lda TP_HEAD+1
    adc #0
    sta __rc19
    ldx #0
    ldy #2
    jsr .Lpt_edge
    lda TP_TAIL
    sta __rc18
    lda TP_TAIL+1
    sta __rc19
    ldx #2
    ldy #4
    jmp .Lpt_edge

; Lexicographic (signed y,signed x) compare, preserving X/Y point indices.
.Lpt_sort_pair:
    lda TP_POINTS+7,x
    eor #$80
    sta TP_SWAP
    lda TP_POINTS+7,y
    eor #$80
    cmp TP_SWAP
    bcc .Lpt_swap
    bne .Lpt_sorted
    lda TP_POINTS+6,y
    cmp TP_POINTS+6,x
    bcc .Lpt_swap
    bne .Lpt_sorted
    lda TP_POINTS+1,x
    eor #$80
    sta TP_SWAP
    lda TP_POINTS+1,y
    eor #$80
    cmp TP_SWAP
    bcc .Lpt_swap
    bne .Lpt_sorted
    lda TP_POINTS,y
    cmp TP_POINTS,x
    bcs .Lpt_sorted
.Lpt_swap:
    lda TP_POINTS,x
    sta TP_SWAP
    lda TP_POINTS,y
    sta TP_POINTS,x
    lda TP_SWAP
    sta TP_POINTS,y
    lda TP_POINTS+1,x
    sta TP_SWAP
    lda TP_POINTS+1,y
    sta TP_POINTS+1,x
    lda TP_SWAP
    sta TP_POINTS+1,y
    lda TP_POINTS+6,x
    sta TP_SWAP
    lda TP_POINTS+6,y
    sta TP_POINTS+6,x
    lda TP_SWAP
    sta TP_POINTS+6,y
    lda TP_POINTS+7,x
    sta TP_SWAP
    lda TP_POINTS+7,y
    sta TP_POINTS+7,x
    lda TP_SWAP
    sta TP_POINTS+7,y
    lda TP_FLAGS
    eor #16
    sta TP_FLAGS
.Lpt_sorted:
    rts

; A/X signed y -> A clamped to 0..200.
.Lpt_clamp:
    cpx #0
    bmi .Lpt_clamp_zero
    bne .Lpt_clamp_bottom
    cmp #200
    bcc .Lpt_clamp_done
.Lpt_clamp_bottom:
    lda #200
.Lpt_clamp_done:
    rts
.Lpt_clamp_zero:
    lda #0
    rts

; Sorted point offsets X/Y; destination rc18/19 receives initial X then step.
.Lpt_edge:
    lda TP_POINTS+7,y
    bmi .Lpt_empty
    ora TP_POINTS+6,y
    beq .Lpt_empty
    lda TP_POINTS+7,x
    bmi .Lpt_start_visible
    bne .Lpt_empty
    lda TP_POINTS+6,x
    cmp #200
    bcs .Lpt_empty
.Lpt_start_visible:
    sec
    lda TP_POINTS+6,y
    sbc TP_POINTS+6,x
    sta TP_DY
    lda TP_POINTS+7,y
    sbc TP_POINTS+7,x
    sta TP_DY+1
    ora TP_DY
    beq .Lpt_empty
    lda TP_POINTS,x
    sta TP_AX
    lda TP_POINTS+1,x
    sta TP_AX+1
    lda TP_POINTS+6,x
    sta TP_AY
    lda TP_POINTS+7,x
    sta TP_AY+1
    sec
    lda TP_POINTS,y
    sbc TP_AX
    sta TP_DX
    lda TP_POINTS+1,y
    sbc TP_AX+1
    sta TP_DX+1
    ora TP_DX
    bne .Lpt_nonvertical
    ; A vertical edge has exact initial ax<<16 and zero slope, with no divide.
    jsr .Lpt_empty
    ldy #2
    lda TP_AX
    sta (__rc18),y
    iny
    lda TP_AX+1
    sta (__rc18),y
    rts
.Lpt_empty:
    lda #0
    ldy #7
.Lpt_zero:
    sta (__rc18),y
    dey
    bpl .Lpt_zero
    rts
.Lpt_nonvertical:
    lda TP_DY
    sta __rc2
    lda TP_DY+1
    sta __rc3
    lda TP_DX
    ldx TP_DX+1
    jsr fast_edge_step
    ldy #4
    sta (__rc18),y
    iny
    txa
    sta (__rc18),y
    iny
    lda __rc2
    sta (__rc18),y
    iny
    lda __rc3
    sta (__rc18),y
    ; Numerator=((max(ay,0)-ay)*2+1)<<14, bounded below 2^28.
    lda TP_AY+1
    bmi .Lpt_negative_start
    lda #0
    sta __rc2
    sta __rc3
    ldx #64
    jmp .Lpt_numerator_ready
.Lpt_negative_start:
    sec
    lda #0
    sbc TP_AY
    sta __rc6
    lda #0
    sbc TP_AY+1
    sta __rc7
    asl __rc6
    rol __rc7
    inc __rc6
    lda __rc6
    and #3
    asl
    asl
    asl
    asl
    asl
    asl
    tax
    lsr __rc7
    ror __rc6
    lsr __rc7
    ror __rc6
    lda __rc6
    sta __rc2
    lda __rc7
    sta __rc3
    lda #0
.Lpt_numerator_ready:
    pha
    lda TP_DY
    sta __rc4
    lda TP_DY+1
    sta __rc5
    pla
    jsr fast_bounded_division
    ; Multiply unsigned t by |dx|, then restore dx's sign. t<32768.
    sta __rc10
    stx __rc11
    lda TP_DX+1
    sta TP_SIGN
    bmi .Lpt_negative_dx
    sta __rc15
    lda TP_DX
    sta __rc14
    jmp .Lpt_magnitude_ready
.Lpt_negative_dx:
    sec
    lda #0
    sbc TP_DX
    sta __rc14
    lda #0
    sbc TP_DX+1
    sta __rc15
.Lpt_magnitude_ready:
    lda #0
    sta __rc6
    sta __rc7
    sta __rc8
    sta __rc9
    sta __rc12
    sta __rc13
.Lpt_multiply:
    lda __rc14
    and #1
    beq .Lpt_multiply_skip
    clc
    lda __rc6
    adc __rc10
    sta __rc6
    lda __rc7
    adc __rc11
    sta __rc7
    lda __rc8
    adc __rc12
    sta __rc8
    lda __rc9
    adc __rc13
    sta __rc9
.Lpt_multiply_skip:
    lsr __rc15
    ror __rc14
    lda __rc14
    ora __rc15
    beq .Lpt_product_ready
    asl __rc10
    rol __rc11
    rol __rc12
    rol __rc13
    jmp .Lpt_multiply
.Lpt_product_ready:
    lda TP_SIGN
    bpl .Lpt_signed_product
    sec
    lda #0
    sbc __rc6
    sta __rc6
    lda #0
    sbc __rc7
    sta __rc7
    lda #0
    sbc __rc8
    sta __rc8
    lda #0
    sbc __rc9
    sta __rc9
.Lpt_signed_product:
    asl __rc6
    rol __rc7
    rol __rc8
    rol __rc9
    clc
    lda __rc8
    adc TP_AX
    sta __rc8
    lda __rc9
    adc TP_AX+1
    sta __rc9
    ldy #0
    lda __rc6
    sta (__rc18),y
    iny
    lda __rc7
    sta (__rc18),y
    iny
    lda __rc8
    sta (__rc18),y
    iny
    lda __rc9
    sta (__rc18),y
    rts
.size fast_prepare_triangle, .-fast_prepare_triangle
