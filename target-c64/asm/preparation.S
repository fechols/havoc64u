; Exact Q16 triangle setup: rc2/3=input16, rc4/5=head16, rc6/7=tail12.
; Input is copied before any output write, including when head aliases input.
; Only caller-save RC2..19 and phase-owned $77A0..$77BE are modified.
; Preparation finishes before row walking and packing reuse this scratch.
; Native positions (long_x, short_x, lower_x) carry +32767 so the row walker
; reads a row's column straight from the high word; steps are unbiased and
; an empty edge stays all zero. Otherwise every byte matches the host
; reference prepare_edge: x0 = (ax << 16) + 2 * dx * t with
; t = ((max(ay, 0) - ay) * 2 + 1) * 16384 / dy and step = dx * 65536 / dy
; toward zero. Edges with dy <= 255 and |dx| <= 255 (nearly all) take a
; 24-bit restoring division by the 8-bit dy, the 16384/dy table when the
; start row is on screen and two quarter-square products; the rest use the
; general 32-bit kernels.
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

.section .phase.shadow.final.preparation,"ax",@progbits
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
    ; Emission accepts only positive-area faces. Sorting the vertices below
    ; still toggles this orientation for each swap.
    lda #16
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
    bpl .Lpt_end_below
    jmp .Lpt_empty
.Lpt_end_below:
    ora TP_POINTS+6,y
    beq .Lpt_empty_near
    lda TP_POINTS+7,x
    bmi .Lpt_start_visible
    bne .Lpt_empty_near
    lda TP_POINTS+6,x
    cmp #200
    bcc .Lpt_start_visible
.Lpt_empty_near:
    jmp .Lpt_empty
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
    ; A vertical edge: exact (ax << 16) + 32767 and zero slope, no divide.
    ldy #0
    lda #$ff
    sta (__rc18),y
    iny
    lda #$7f
    sta (__rc18),y
    iny
    lda TP_AX
    sta (__rc18),y
    iny
    lda TP_AX+1
    sta (__rc18),y
    lda #0
    iny
    sta (__rc18),y
    iny
    sta (__rc18),y
    iny
    sta (__rc18),y
    iny
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
    ; |dx| (rc14 low, rc9 high) and dx's sign
    lda TP_DX+1
    sta TP_SIGN
    bpl .Lpt_dx_positive
    sec
    lda #0
    sbc TP_DX
    sta __rc14
    lda #0
    sbc TP_DX+1
    sta __rc9
    jmp .Lpt_dx_ready
.Lpt_dx_positive:
    lda TP_DX
    sta __rc14
    lda TP_DX+1
    sta __rc9
.Lpt_dx_ready:
    lda TP_DY+1
    ora __rc9
    beq .Lpt_fast
    jmp .Lpt_general
.Lpt_fast:
    ; step magnitude = (|dx| << 16) / dy: in-place restoring division of the
    ; 24-bit dividend rc2..rc4 (quotient bits enter at the bottom) with the
    ; remainder in A; the dividend's leading zero bits are skipped up front.
    lda TP_DY
    sta __rc15
    lda #0
    sta __rc2
    sta __rc3
    ldx #24
    lda __rc14
    bmi .Lpt_divide
.Lpt_lead:
    dex
    asl
    bpl .Lpt_lead
.Lpt_divide:
    sta __rc4
    lda #0
.Lpt_div_bit:
    asl __rc2
    rol __rc3
    rol __rc4
    rol
    bcs .Lpt_div_sub
    cmp __rc15
    bcc .Lpt_div_next
.Lpt_div_sub:
    sbc __rc15
    inc __rc2
.Lpt_div_next:
    dex
    bne .Lpt_div_bit
    ldy #4
    lda TP_SIGN
    bmi .Lpt_step_negative
    lda __rc2
    sta (__rc18),y
    iny
    lda __rc3
    sta (__rc18),y
    iny
    lda __rc4
    sta (__rc18),y
    iny
    lda #0
    sta (__rc18),y
    beq .Lpt_fraction
.Lpt_step_negative:
    sec
    lda #0
    sbc __rc2
    sta (__rc18),y
    iny
    lda #0
    sbc __rc3
    sta (__rc18),y
    iny
    lda #0
    sbc __rc4
    sta (__rc18),y
    iny
    lda #0
    sbc #0
    sta (__rc18),y
.Lpt_fraction:
    ; t = 16384 / dy from the table while the start row is on screen,
    ; otherwise the bounded division of ((-ay) * 2 + 1) << 14 (t < 32768)
    lda TP_AY+1
    bmi .Lpt_t_divided
    ldx TP_DY
    lda .Lpt_t16384_lo,x
    sta __rc6
    lda .Lpt_t16384_hi,x
    sta __rc7
    jmp .Lpt_t_ready
.Lpt_t_divided:
    jsr .Lpt_t_division
    sta __rc6
    stx __rc7
.Lpt_t_ready:
    ; P = |dx| * t in rc8, rc9, rc15 (|dx| and t_hi both fit a byte)
    ldx __rc14
    ldy __rc6
    jsr fast_mul8
    sta __rc8
    stx __rc9
    lda #0
    sta __rc15
    ldy __rc7
    beq .Lpt_product_doubled
    ldx __rc14
    jsr fast_mul8
    clc
    adc __rc9
    sta __rc9
    txa
    adc #0
    sta __rc15
.Lpt_product_doubled:
    asl __rc8
    rol __rc9
    rol __rc15
    ; x0 = (ax << 16) + 32767 +/- 2P
    ldy #0
    lda TP_SIGN
    bmi .Lpt_x0_negative
    clc
    lda #$ff
    adc __rc8
    sta (__rc18),y
    iny
    lda #$7f
    adc __rc9
    sta (__rc18),y
    iny
    lda TP_AX
    adc __rc15
    sta (__rc18),y
    iny
    lda TP_AX+1
    adc #0
    sta (__rc18),y
    rts
.Lpt_x0_negative:
    sec
    lda #$ff
    sbc __rc8
    sta (__rc18),y
    iny
    lda #$7f
    sbc __rc9
    sta (__rc18),y
    iny
    lda TP_AX
    sbc __rc15
    sta (__rc18),y
    iny
    lda TP_AX+1
    sbc #0
    sta (__rc18),y
    rts

; t for a start row above the screen: ((-ay) * 2 + 1) << 14 divided by dy,
; the numerator bounded below 2^28 so the bounded division applies. A/X = t.
.Lpt_t_division:
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
    pha
    lda TP_DY
    sta __rc4
    lda TP_DY+1
    sta __rc5
    pla
    jmp fast_bounded_division

; General edge: 32-bit slope kernel, t by the bounded division, the product
; by shifted adds. Reached when dy or |dx| exceeds 255.
.Lpt_general:
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
    ; + 32767: the native position bias
    clc
    lda __rc6
    adc #$ff
    ldy #0
    sta (__rc18),y
    iny
    lda __rc7
    adc #$7f
    sta (__rc18),y
    iny
    lda __rc8
    adc #0
    sta (__rc18),y
    iny
    lda __rc9
    adc #0
    sta (__rc18),y
    rts
.size fast_prepare_triangle, .-fast_prepare_triangle

; t for the row centre: 16384 / dy for dy = 1..255 (entry 0 unused).
.Lpt_t16384_lo:
    .byte 0
.set .Lpt_i, 1
.rept 255
    .byte (16384 / .Lpt_i) & 255
.set .Lpt_i, .Lpt_i + 1
.endr
.Lpt_t16384_hi:
    .byte 0
.set .Lpt_i, 1
.rept 255
    .byte (16384 / .Lpt_i) >> 8
.set .Lpt_i, .Lpt_i + 1
.endr
