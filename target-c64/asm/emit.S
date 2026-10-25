; void fast_emit_triangle(const Vec3 points[3], uint8_t color, uint8_t flags)
; C ABI: points rc2/3 (camera-space x, y, z as i16, all z >= NEAR), color A,
; flags X. The native body of Renderer::project_triangle for the single
; resident Renderer at RENDERER (header: triangle_count u16, dropped u16,
; clear_color, padding; commands of 16 bytes from RENDERER+6 = $D000):
;   project the three corners through the 64-entry vertex cache at $8E00
;     (entry: point x, y, z then screen x, y; a byte-mix index — the cache
;     is a memo keyed by the whole point, so the host's arithmetic hash in
;     projection::project_cached need not be reproduced; misses call
;     fast_project in place);
;   area = (x1-x0)(y2-y0) - (y1-y0)(x2-x0); nonpositive -> nothing;
;   all x <= 0, all x >= 160, all y <= 0 or all y >= 200 -> nothing;
;   256 commands -> dropped += 1 (saturating) -> nothing;
;   depth = (z0 + z1 + z2) / 3 (exact, by the 85 h + (h + l) / 3 identity);
;   command = x[3], y[3], depth, color & 15,
;             flags & (STATIC_SKY | TERRAIN_CLEAR | EXTERIOR = $62);
;   triangle_count += 1.
; Everything after the projection is the shared .Let_emit tail, which the
; mesh kernel below enters with projected corners of its own.
; Scratch at $9358-$93CF is free RAM no interrupt touches.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.set CACHE, $8e00
.set ET_POINTS, $9358
.set ET_COLOR, $935a
.set ET_FLAGS, $935b
.set ET_CORNER, $935c
.set ET_V, $935d
.set ET_ENTRY, $9363
.set ET_HASH, $9365
.set ET_SX, $9367
.set ET_SY, $936d
.set ET_A, $9373
.set ET_B, $9375
.set ET_NEG, $9377
.set ET_R, $9378
.set ET_P1, $937c
.set ET_SUM, $9380
.set ET_COMMAND, $9384

.section .phase.geometry,"ax",@progbits
.globl fast_emit_triangle
.type fast_emit_triangle,@function
fast_emit_triangle:
    sta ET_COLOR
    stx ET_FLAGS
    lda __rc2
    sta ET_POINTS
    lda __rc3
    sta ET_POINTS+1
    ; depth sum z0 + z1 + z2 (24 bits) before the corners are consumed
    ldy #4
    lda (__rc2),y
    sta ET_SUM
    iny
    lda (__rc2),y
    sta ET_SUM+1
    lda #0
    sta ET_SUM+2
    ldy #10
    clc
    lda (__rc2),y
    adc ET_SUM
    sta ET_SUM
    iny
    lda (__rc2),y
    adc ET_SUM+1
    sta ET_SUM+1
    bcc .Let_sum_third
    inc ET_SUM+2
.Let_sum_third:
    ldy #16
    clc
    lda (__rc2),y
    adc ET_SUM
    sta ET_SUM
    iny
    lda (__rc2),y
    adc ET_SUM+1
    sta ET_SUM+1
    bcc .Let_summed
    inc ET_SUM+2
.Let_summed:
    lda #0
    sta ET_CORNER
.Let_corner:
    ; rc2/3 point at this corner (x, y, z). Cache index: a byte mix of the
    ; six coordinate bytes (a pure memo: any index works, this one keeps the
    ; miss rate of the host's arithmetic hash at a fraction of its cost):
    ;   ((x_lo >> 2) ^ (x_hi << 4) ^ y_lo ^ (y_hi << 1) ^ (z_lo >> 1) ^ (z_hi << 2)) & 63
    ldy #0
    lda (__rc2),y
    lsr
    lsr
    sta ET_HASH
    iny
    lda (__rc2),y
    asl
    asl
    asl
    asl
    eor ET_HASH
    iny
    eor (__rc2),y
    sta ET_HASH
    iny
    lda (__rc2),y
    asl
    eor ET_HASH
    sta ET_HASH
    iny
    lda (__rc2),y
    lsr
    eor ET_HASH
    sta ET_HASH
    iny
    lda (__rc2),y
    asl
    asl
    eor ET_HASH
    and #63
    tax
    lda .Let_entry_lo,x
    sta __rc4
    lda .Let_entry_hi,x
    sta __rc5
    ; compare the cached point with the corner, depth bytes first
    ldy #5
.Let_compare:
    lda (__rc4),y
    cmp (__rc2),y
    bne .Let_miss
    dey
    bpl .Let_compare
    bmi .Let_hit
.Let_miss:
    ldy #5
.Let_fill:
    lda (__rc2),y
    sta (__rc4),y
    dey
    bpl .Let_fill
    ; fast_project(&entry.point, &entry.screen)
    lda __rc4
    sta __rc2
    lda __rc5
    sta __rc3
    clc
    lda __rc4
    adc #6
    sta __rc4
    lda __rc5
    adc #0
    sta __rc5
    jsr fast_project
    sec
    lda __rc4
    sbc #6
    sta __rc4
    lda __rc5
    sbc #0
    sta __rc5
.Let_hit:
    lda ET_CORNER
    asl
    tax                     ; corner * 2
    ldy #6
    lda (__rc4),y
    sta ET_SX,x
    iny
    lda (__rc4),y
    sta ET_SX+1,x
    iny
    lda (__rc4),y
    sta ET_SY,x
    iny
    lda (__rc4),y
    sta ET_SY+1,x
    ; next corner: six bytes on
    clc
    lda ET_POINTS
    adc #6
    sta ET_POINTS
    sta __rc2
    lda ET_POINTS+1
    adc #0
    sta ET_POINTS+1
    sta __rc3
    inc ET_CORNER
    lda ET_CORNER
    cmp #3
    beq .Let_emit
    jmp .Let_corner; Shared tail: ET_SX/ET_SY hold the three projected corners, ET_SUM the
; 24-bit depth sum, ET_COLOR and ET_FLAGS the face's colour and flags.
.Let_emit:
    ; P1 = (x1 - x0) * (y2 - y0)
    sec
    lda ET_SX+2
    sbc ET_SX
    sta ET_A
    lda ET_SX+3
    sbc ET_SX+1
    sta ET_A+1
    sec
    lda ET_SY+4
    sbc ET_SY
    sta ET_B
    lda ET_SY+5
    sbc ET_SY+1
    sta ET_B+1
    jsr .Let_mul16
    ldx #3
.Let_keep_p1:
    lda ET_R,x
    sta ET_P1,x
    dex
    bpl .Let_keep_p1
    ; P2 = (y1 - y0) * (x2 - x0)
    sec
    lda ET_SY+2
    sbc ET_SY
    sta ET_A
    lda ET_SY+3
    sbc ET_SY+1
    sta ET_A+1
    sec
    lda ET_SX+4
    sbc ET_SX
    sta ET_B
    lda ET_SX+5
    sbc ET_SX+1
    sta ET_B+1
    jsr .Let_mul16
    ; area = P1 - P2, kept in ET_P1 (unrolled: cpx would clobber the borrow)
    sec
    lda ET_P1
    sbc ET_R
    sta ET_P1
    lda ET_P1+1
    sbc ET_R+1
    sta ET_P1+1
    lda ET_P1+2
    sbc ET_R+2
    sta ET_P1+2
    lda ET_P1+3
    sbc ET_R+3
    sta ET_P1+3
    lda ET_P1
    ora ET_P1+1
    ora ET_P1+2
    ora ET_P1+3
    beq .Let_done
    lda ET_P1+3
    bpl .Let_oriented
.Let_done:
    rts
.Let_oriented:
    ; Reject triangles entirely off one screen edge.
    ldx #0
.Let_left:
    lda ET_SX+1,x
    bmi .Let_left_next
    ora ET_SX,x
    bne .Let_not_left
.Let_left_next:
    inx
    inx
    cpx #6
    bne .Let_left
    beq .Let_done
.Let_not_left:
    ldx #0
.Let_right:
    lda ET_SX+1,x
    bmi .Let_not_right
    bne .Let_right_next
    lda ET_SX,x
    cmp #160
    bcc .Let_not_right
.Let_right_next:
    inx
    inx
    cpx #6
    bne .Let_right
    beq .Let_done
.Let_not_right:
    ldx #0
.Let_top:
    lda ET_SY+1,x
    bmi .Let_top_next
    ora ET_SY,x
    bne .Let_not_top
.Let_top_next:
    inx
    inx
    cpx #6
    bne .Let_top
    beq .Let_done
.Let_not_top:
    ldx #0
.Let_bottom:
    lda ET_SY+1,x
    bmi .Let_not_bottom
    bne .Let_bottom_next
    lda ET_SY,x
    cmp #200
    bcc .Let_not_bottom
.Let_bottom_next:
    inx
    inx
    cpx #6
    bne .Let_bottom
    beq .Let_done
.Let_not_bottom:
    ; capacity
    lda RENDERER+1
    beq .Let_room
    lda RENDERER+2
    cmp #$ff
    bne .Let_drop
    lda RENDERER+3
    cmp #$ff
    beq .Let_done
.Let_drop:
    inc RENDERER+2
    bne .Let_room_check_done
    inc RENDERER+3
.Let_room_check_done:
    rts
.Let_room:
    ; depth = (z0 + z1 + z2) / 3, exactly, without a division: with the sum
    ; S = 256 h + l (h = s2:s1 <= 383), S / 3 = 85 h + (h + l) / 3, and with
    ; m = h + l = 256 c + d (c <= 2), m / 3 = 85 c + (c + d) / 3 where
    ; c + d <= 257 comes from the 258-entry thirds table.
    ldx ET_SUM+1
    ldy #85
    jsr fast_mul8
    sta ET_A
    stx ET_A+1              ; 85 s1
    lda ET_SUM+2
    beq .Let_third_low
    clc
    lda ET_A
    adc #<21760
    sta ET_A
    lda ET_A+1
    adc #>21760
    sta ET_A+1              ; + 85 * 256 s2
.Let_third_low:
    ; m = s1 + l + 256 s2: d = m low byte, c = m high byte (0..2)
    clc
    lda ET_SUM+1
    adc ET_SUM
    tay                     ; d
    lda ET_SUM+2
    adc #0
    tax                     ; c
    sta ET_B
    tya
    clc
    adc ET_B                ; c + d, carry when it passed 255
    bcs .Let_third_wrap
    tay
    lda .Let_thirds,y
    clc
    adc .Let_thirds_85,x    ; + 85 c
    jmp .Let_third_done
.Let_third_wrap:
    ; c + d = 256 + e: floor((256 + e) / 3) = 85 + floor((e + 1) / 3)
    tay
    iny
    lda .Let_thirds,y
    clc
    adc #85
    adc .Let_thirds_85,x
.Let_third_done:
    clc
    adc ET_A
    sta ET_SUM
    lda ET_A+1
    adc #0
    sta ET_SUM+1
    ; command address = RENDERER + 6 + count * 16 (count < 256 here)
    lda RENDERER
    lsr
    lsr
    lsr
    lsr
    clc
    adc #>(RENDERER+6)
    sta __rc5
    lda RENDERER
    asl
    asl
    asl
    asl
    clc
    adc #<(RENDERER+6)
    sta __rc4
    bcc .Let_command_ready
    inc __rc5
.Let_command_ready:
    ldy #0
.Let_write_screen:
    lda ET_SX,y
    sta (__rc4),y
    iny
    cpy #12
    bne .Let_write_screen
    lda ET_SUM
    sta (__rc4),y
    iny
    lda ET_SUM+1
    sta (__rc4),y
    iny
    lda ET_COLOR
    and #15
    sta (__rc4),y
    iny
    lda ET_FLAGS
    and #$62
    sta (__rc4),y
    inc RENDERER
    bne .Let_counted
    inc RENDERER+1
.Let_counted:
    rts

; ET_R (32-bit) = ET_A * ET_B, both signed 16-bit.
.Let_mul16:
    lda ET_A+1
    eor ET_B+1
    sta ET_NEG
    lda ET_A+1
    bpl .Let_mul16_b
    sec
    lda #0
    sbc ET_A
    sta ET_A
    lda #0
    sbc ET_A+1
    sta ET_A+1
.Let_mul16_b:
    lda ET_B+1
    bpl .Let_mul16_go
    sec
    lda #0
    sbc ET_B
    sta ET_B
    lda #0
    sbc ET_B+1
    sta ET_B+1
.Let_mul16_go:
    ldx ET_A
    ldy ET_B
    jsr fast_mul8
    sta ET_R
    stx ET_R+1
    lda #0
    sta ET_R+2
    sta ET_R+3
    lda ET_A+1
    beq .Let_mul16_high_b
    tax
    ldy ET_B
    jsr fast_mul8
    jsr .Let_mul16_middle
.Let_mul16_high_b:
    lda ET_B+1
    beq .Let_mul16_sign
    tay
    ldx ET_A
    jsr fast_mul8
    jsr .Let_mul16_middle
    lda ET_A+1
    beq .Let_mul16_sign
    tax
    ldy ET_B+1
    jsr fast_mul8
    clc
    adc ET_R+2
    sta ET_R+2
    txa
    adc ET_R+3
    sta ET_R+3
.Let_mul16_sign:
    bit ET_NEG
    bpl .Let_mul16_done
    sec
    lda #0
    sbc ET_R
    sta ET_R
    lda #0
    sbc ET_R+1
    sta ET_R+1
    lda #0
    sbc ET_R+2
    sta ET_R+2
    lda #0
    sbc ET_R+3
    sta ET_R+3
.Let_mul16_done:
    rts
.Let_mul16_middle:
    clc
    adc ET_R+1
    sta ET_R+1
    txa
    adc ET_R+2
    sta ET_R+2
    bcc .Let_mul16_middle_done
    inc ET_R+3
.Let_mul16_middle_done:
    rts
; floor(n / 3) for n = 0..255, and 85 c for c = 0..2 (the depth third).
.Let_thirds:
.set .Let_i, 0
.rept 256
    .byte .Let_i / 3
.set .Let_i, .Let_i + 1
.endr
.Let_thirds_85:
    .byte 0, 85, 170
; the 64 cache entries: CACHE + 10 * index
.Let_entry_lo:
.set .Let_i, 0
.rept 64
    .byte <(CACHE + 10 * .Let_i)
.set .Let_i, .Let_i + 1
.endr
.Let_entry_hi:
.set .Let_i, 0
.rept 64
    .byte >(CACHE + 10 * .Let_i)
.set .Let_i, .Let_i + 1
.endr
.size fast_emit_triangle, .-fast_emit_triangle

; void fast_clip_triangle(const Vec3 points[3], uint8_t color, uint8_t flags)
; C ABI: points rc2/3, color A, flags X. The native body of the renderer's
; GeometrySink::triangle: corners all at or behind the near plane (z >= 8)
; go straight to fast_emit_triangle; corners all in front are dropped; a
; mixed triangle is clipped against the plane exactly like the host — walking
; the corners a, b, c with c as the first predecessor, an edge that crosses
; the plane contributes near_intersection(previous, current), a corner behind
; the plane contributes itself — and the three- or four-corner polygon is
; emitted as (p0, p1, p2) and, for four, (p0, p2, p3). near_intersection
; orders its two points by depth (a nearer), then t = ((8 - a.z) << 15) /
; (b.z - a.z) with the bounded division (t <= 32768) and x = a.x +
; ((b.x - a.x) * t >> 15) with the host's arithmetic shift (the 17-bit
; difference times t through four quarter-square partials, a negative product
; rounded toward minus infinity), likewise y; z = 8. Scratch at $93D0-$93FF.
.set CL_POLY, $93d0
.set CL_LEN, $93fa
.set CL_IN, $93fb
.set CL_COLOR, $93fe
.set CL_FLAGS, $93ff
.set CL_SRC, ET_POINTS
.set CL_A, ET_SX
.set CL_B, ET_SY
.set CL_T, ET_A
.set CL_D, ET_R
.set CL_M, ET_P1
.set CL_NEG, ET_NEG
.set CL_DST, ET_B
.globl fast_clip_triangle
.type fast_clip_triangle,@function
fast_clip_triangle:
    sta CL_COLOR
    stx CL_FLAGS
    ; inside flags: z >= 8 <=> z_hi positive and nonzero, or z_hi zero and z_lo >= 8
    ldy #17
    ldx #2
.Lcl_classify:
    lda (__rc2),y
    bmi .Lcl_outside
    bne .Lcl_inside
    dey
    lda (__rc2),y
    iny
    cmp #8
    bcs .Lcl_inside
.Lcl_outside:
    lda #0
    beq .Lcl_classified
.Lcl_inside:
    lda #1
.Lcl_classified:
    sta CL_IN,x
    dey
    dey
    dey
    dey
    dey
    dey
    dex
    bpl .Lcl_classify
    lda CL_IN
    and CL_IN+1
    and CL_IN+2
    beq .Lcl_not_all_inside
    lda CL_COLOR
    ldx CL_FLAGS
    jmp fast_emit_triangle
.Lcl_not_all_inside:
    lda CL_IN
    ora CL_IN+1
    ora CL_IN+2
    bne .Lcl_mixed
    rts
.Lcl_mixed:
    lda __rc2
    sta CL_SRC
    lda __rc3
    sta CL_SRC+1
    lda #0
    sta CL_LEN
    ; previous = corner 2, then current = 0, 1, 2
    ldx #2
    stx ET_CORNER
    ldx #0
.Lcl_walk:
    stx ET_V                ; current corner index
    ldy ET_CORNER           ; previous corner index
    lda CL_IN,x
    cmp CL_IN,y
    beq .Lcl_no_crossing
    jsr .Lcl_intersection
    ldx ET_V
.Lcl_no_crossing:
    lda CL_IN,x
    beq .Lcl_walk_next
    ; copy corner X into the polygon
    txa
    asl
    adc ET_V
    asl                     ; X * 6
    tay
    lda CL_LEN
    asl
    adc CL_LEN
    asl                     ; LEN * 6
    tax
    lda #6
    sta ET_V+1
.Lcl_copy_corner:
    lda (__rc2),y
    sta CL_POLY,x
    iny
    inx
    dec ET_V+1
    bne .Lcl_copy_corner
    inc CL_LEN
    ldx ET_V
.Lcl_walk_next:
    stx ET_CORNER
    inx
    cpx #3
    bne .Lcl_walk
    ; emit (p0, p1, p2), then (p0, p2, p3) rebuilt contiguously after p3
    lda #<CL_POLY
    sta __rc2
    lda #>CL_POLY
    sta __rc3
    lda CL_COLOR
    ldx CL_FLAGS
    jsr fast_emit_triangle
    lda CL_LEN
    cmp #4
    bne .Lcl_done
    ldx #0
.Lcl_second:
    lda CL_POLY,x
    sta CL_POLY+24,x
    lda CL_POLY+12,x
    sta CL_POLY+30,x
    lda CL_POLY+18,x
    sta CL_POLY+36,x
    inx
    cpx #6
    bne .Lcl_second
    lda #<(CL_POLY+24)
    sta __rc2
    lda #>(CL_POLY+24)
    sta __rc3
    lda CL_COLOR
    ldx CL_FLAGS
    jmp fast_emit_triangle
.Lcl_done:
    rts

; The intersection of edge (corner Y = previous, corner X = current) with the
; near plane, appended to the polygon. Copies the nearer point to CL_A and the
; farther to CL_B first (both owners of a shared edge round alike).
.Lcl_intersection:
    txa
    asl
    adc ET_V
    asl
    sta ET_V+2              ; current * 6
    tya
    asl
    adc ET_CORNER
    asl
    sta ET_V+3              ; previous * 6
    ; the nearer point (smaller z) is a: compare the two depths, signed
    ldy ET_V+2
    iny
    iny
    iny
    iny
    lda (__rc2),y           ; current z low
    sta ET_V+4
    iny
    lda (__rc2),y           ; current z high
    ldy ET_V+3
    iny
    iny
    iny
    iny
    iny
    sta ET_V+5
    lda (__rc2),y           ; previous z high
    tax
    dey
    lda (__rc2),y           ; previous z low
    ; previous < current? compute previous - current (signed compare)
    cmp ET_V+4
    txa
    sbc ET_V+5
    bvc .Lcl_compare_done
    eor #$80
.Lcl_compare_done:
    bmi .Lcl_previous_nearer
    ; current is a (nearer or equal), previous is b
    ldy ET_V+2
    ldx #0
    jsr .Lcl_copy_a
    ldy ET_V+3
    jsr .Lcl_copy_b
    jmp .Lcl_interpolate
.Lcl_previous_nearer:
    ldy ET_V+3
    ldx #0
    jsr .Lcl_copy_a
    ldy ET_V+2
    jsr .Lcl_copy_b
.Lcl_interpolate:
    ; t = ((8 - a.z) << 15) / (b.z - a.z): numerator high word = (8 - a.z) >> 1,
    ; low word = (8 - a.z) & 1 ? $8000 : 0
    sec
    lda #8
    sbc CL_A+4
    sta CL_T
    lda #0
    sbc CL_A+5
    sta CL_T+1              ; 8 - a.z (1..32776, 17 bits at most -> positive)
    lsr CL_T+1
    ror CL_T
    lda CL_T
    sta __rc2
    lda CL_T+1
    sta __rc3
    lda #0
    ror                     ; the shifted-out bit into bit 7 (A = $80 or 0)
    tax
    sec
    lda CL_B+4
    sbc CL_A+4
    sta __rc4
    lda CL_B+5
    sbc CL_A+5
    sta __rc5
    lda #0
    jsr fast_bounded_division
    sta CL_T
    stx CL_T+1
    ; x then y: a + ((b - a) * t >> 15)
    ldx #0
    jsr .Lcl_axis
    ldx #2
    jsr .Lcl_axis
    ; z = 8; append
    lda #8
    sta CL_A+4
    lda #0
    sta CL_A+5
    lda CL_LEN
    asl
    adc CL_LEN
    asl
    tax
    ldy #0
.Lcl_append:
    lda CL_A,y
    sta CL_POLY,x
    inx
    iny
    cpy #6
    bne .Lcl_append
    inc CL_LEN
    lda CL_SRC
    sta __rc2
    lda CL_SRC+1
    sta __rc3
    rts
; Copy the six bytes of corner Y into CL_A (X = 0) or CL_B (X = 6).
.Lcl_copy_a:
.Lcl_copy_b:
    lda #6
    sta ET_V+1
.Lcl_copy_point:
    lda (__rc2),y
    sta CL_A,x
    iny
    inx
    dec ET_V+1
    bne .Lcl_copy_point
    rts
; CL_A[X] += ((CL_B[X] - CL_A[X]) * t) >> 15 for axis offset X (0 = x, 2 = y).
.Lcl_axis:
    stx CL_DST
    sec
    lda CL_B,x
    sbc CL_A,x
    sta CL_D
    lda CL_B+1,x
    sbc CL_A+1,x
    sta CL_D+1
    ; third byte of the 17-bit difference: signext(b) - signext(a) - borrow
    ; (loads and branches leave the carry of the subtraction intact)
    lda CL_B+1,x
    bpl .Lcl_b_positive
    lda #$ff
    bne .Lcl_b_extended
.Lcl_b_positive:
    lda #0
.Lcl_b_extended:
    sta CL_D+2
    lda CL_A+1,x
    bpl .Lcl_a_positive
    lda #$ff
    bne .Lcl_a_extended
.Lcl_a_positive:
    lda #0
.Lcl_a_extended:
    sta CL_NEG
    lda CL_D+2
    sbc CL_NEG
    sta CL_D+2              ; 0 or $ff
    sta CL_NEG
    beq .Lcl_magnitude
    sec
    lda #0
    sbc CL_D
    sta CL_D
    lda #0
    sbc CL_D+1
    sta CL_D+1
    lda #0
    sbc CL_D+2
    sta CL_D+2              ; the magnitude fits sixteen bits: 0
.Lcl_magnitude:
    ; M = |d| * t: d0 t0 + (d0 t1 + d1 t0) << 8 + d1 t1 << 16 + (d2 ? t << 16 : 0)
    ldx CL_D
    ldy CL_T
    jsr fast_mul8
    sta CL_M
    stx CL_M+1
    lda #0
    sta CL_M+2
    sta CL_M+3
    ldx CL_D
    ldy CL_T+1
    jsr fast_mul8
    jsr .Lcl_add_middle
    ldx CL_D+1
    beq .Lcl_high_byte
    ldy CL_T
    jsr fast_mul8
    jsr .Lcl_add_middle
    ldx CL_D+1
    ldy CL_T+1
    jsr fast_mul8
    clc
    adc CL_M+2
    sta CL_M+2
    txa
    adc CL_M+3
    sta CL_M+3
.Lcl_high_byte:
    lda CL_D+2
    beq .Lcl_shift
    clc
    lda CL_M+2
    adc CL_T
    sta CL_M+2
    lda CL_M+3
    adc CL_T+1
    sta CL_M+3
.Lcl_shift:
    ; q = M >> 15 for a positive difference; -((M + 32767) >> 15) otherwise
    bit CL_NEG
    bpl .Lcl_floor
    clc
    lda CL_M
    adc #$ff
    lda CL_M+1
    adc #$7f
    sta CL_M+1
    lda CL_M+2
    adc #0
    sta CL_M+2
    lda CL_M+3
    adc #0
    sta CL_M+3
.Lcl_floor:
    ; M >> 15 = (M >> 16) << 1 | bit 15 of M: take bytes 1..3 and shift right 7
    lda CL_M+1
    asl                     ; bit 7 (bit 15 of M) into the carry
    lda CL_M+2
    rol
    sta CL_D
    lda CL_M+3
    rol
    sta CL_D+1              ; q (positive, 17 bits at most -> fits: |q| <= 65535)
    bit CL_NEG
    bpl .Lcl_add
    sec
    lda #0
    sbc CL_D
    sta CL_D
    lda #0
    sbc CL_D+1
    sta CL_D+1
.Lcl_add:
    ldx CL_DST
    clc
    lda CL_A,x
    adc CL_D
    sta CL_A,x
    lda CL_A+1,x
    adc CL_D+1
    sta CL_A+1,x
    rts
.Lcl_add_middle:
    clc
    adc CL_M+1
    sta CL_M+1
    txa
    adc CL_M+2
    sta CL_M+2
    bcc .Lcl_add_middle_done
    inc CL_M+3
.Lcl_add_middle_done:
    rts
.size fast_clip_triangle, .-fast_clip_triangle

; uint8_t fast_replay_scenery(uint8_t start, uint8_t count, int16_t parallax_x, int16_t parallax_y)
; C ABI: start A, count X, parallax_x rc2/3, parallax_y rc4/5; returns in A.
; The native loop of Renderer::project_scenery: records of 20 bytes (three
; camera-space points, colour, flags) staged at RENDERER + 6 + 2176 are
; consumed in order from `start`. A STATIC_SKY record (flags bit 1) first has
; the sky parallax subtracted from its points in place, twice when its XYZ
; were doubled for a farther layer (staging-only flags bit 2); a record whose three
; depths all reach the near plane (z >= 8) is emitted with fast_emit_triangle;
; the first record that needs near clipping stops the loop and its index is
; returned (already drifted) so the caller clips it and resumes at index + 1.
; Returns `count` when every record was consumed. Commands written so far
; never reach an unread record (32 k < 2176 + 20 k), so no copy is needed.
.set RECORDS, RENDERER + 6 + 2176
.set RS_INDEX, $9388
.set RS_COUNT, $9389
.set RS_PX, $938a
.set RS_PY, $938c
.set RS_PTR, $938e
.set RS_T, $9390
.set RS_FLAGS, $9392
.globl fast_replay_scenery
.type fast_replay_scenery,@function
fast_replay_scenery:
    sta RS_INDEX
    stx RS_COUNT
    lda __rc2
    sta RS_PX
    lda __rc3
    sta RS_PX+1
    lda __rc4
    sta RS_PY
    lda __rc5
    sta RS_PY+1
.Lrs_next:
    lda RS_INDEX
    cmp RS_COUNT
    bcc .Lrs_record
    rts
.Lrs_record:
    ; record address = RECORDS + index * 20 = RECORDS + index * 16 + index * 4
    sta RS_PTR
    lda #0
    sta RS_PTR+1
    asl RS_PTR
    rol RS_PTR+1
    asl RS_PTR
    rol RS_PTR+1
    lda RS_PTR
    sta RS_T
    lda RS_PTR+1
    sta RS_T+1
    asl RS_PTR
    rol RS_PTR+1
    asl RS_PTR
    rol RS_PTR+1
    clc
    lda RS_PTR
    adc RS_T
    sta RS_PTR
    lda RS_PTR+1
    adc RS_T+1
    sta RS_PTR+1
    clc
    lda RS_PTR
    adc #<RECORDS
    sta __rc2
    lda RS_PTR+1
    adc #>RECORDS
    sta __rc3
    ldy #19
    lda (__rc2),y
    sta RS_FLAGS
    and #2
    tax                         ; repeat toggle starts with bit 2 clear
    beq .Lrs_near
    ; Backdrop: x -= parallax_x, y -= parallax_y for the three points.
.Lrs_drift_start:
    ldy #0
.Lrs_drift:
    sec
    lda (__rc2),y
    sbc RS_PX
    sta (__rc2),y
    iny
    lda (__rc2),y
    sbc RS_PX+1
    sta (__rc2),y
    iny
    sec
    lda (__rc2),y
    sbc RS_PY
    sta (__rc2),y
    iny
    lda (__rc2),y
    sbc RS_PY+1
    sta (__rc2),y
    iny
    iny
    iny
    cpy #18
    bne .Lrs_drift
    ; Double-depth backdrops retain their original projected camera drift.
    ; The drift body preserves X. Toggle its bit 2 against the modifier: a
    ; distant layer repeats once, then clears the toggle on its second pass.
    ; Neither the shared frame offsets nor staged flags are changed.
    txa
    eor RS_FLAGS
    and #4
    tax
    bne .Lrs_drift_start
.Lrs_near:
    ldy #5
    jsr .Lrs_depth_ok
    bcc .Lrs_clip
    ldy #11
    jsr .Lrs_depth_ok
    bcc .Lrs_clip
    ldy #17
    jsr .Lrs_depth_ok
    bcc .Lrs_clip
    ldy #18
    lda (__rc2),y
    ldx RS_FLAGS
    jsr fast_emit_triangle
    inc RS_INDEX
    jmp .Lrs_next
.Lrs_clip:
    lda RS_INDEX
    rts
; Y = offset of a depth's high byte; carry set when that depth is >= 8.
.Lrs_depth_ok:
    lda (__rc2),y
    bmi .Lrs_depth_behind
    bne .Lrs_depth_far
    dey
    lda (__rc2),y
    cmp #8
    rts
.Lrs_depth_far:
    sec
    rts
.Lrs_depth_behind:
    clc
    rts
.size fast_replay_scenery, .-fast_replay_scenery
; uint8_t fast_emit_mesh(MeshArgs *args, uint8_t start)
; C ABI: args rc2/3, start A; returns in A. The native body of the scene
; sink's mesh path (SceneBudget::mesh): one mesh instance is transformed once
; and its faces index the results. MeshArgs (repr(C), 23 bytes):
;    0 vertices   *const i8[3]      stored vertex bytes
;    2 faces      *const u8[4]      three indices and a colour per face
;    4 matrix     *const i16[9]     row-major Q8 rotation
;    6 position   Vec3              model origin in world space
;   12 camera_y   i16               chase camera height
;   14 vertex_count, 15 face_count, 16 shift (RotationMatrix::apply, 5..9)
;   17 flags                        the sink's scene metadata flags
;   18 color                        colour override (0 = the face's own)
;   19 face_color (out), 20 indices[3] (out): the face handed back for clipping
; With start = 0 every vertex is rotated with fast_rotate_vertex, translated
; with saturating adds, lowered by camera_y and stored as a 16-byte record at
; $E000 + 16 * index (x, y, z, screen x, screen y, projected flag): the raster
; tails live there but are idle until PIXELS. Every face runs from `start`:
; a face with all three
; depths at or behind the near plane (z >= 8) projects any corner not yet
; projected and enters .Let_emit; one entirely in front is dropped; a mixed
; face stops the walk and its index is returned with its colour and indices
; written back so the caller clips it and resumes at index + 1. face_count
; is returned only when the walk is complete. The bounded renderer counts
; surviving commands and physical overflow; the caller may roll back the whole
; mesh and substitute a proxy after checking its projected command budget.
.set RECORDS_BASE, $e000
.set EM_ARGS, $93a0
.set EM_VERT, EM_ARGS+0
.set EM_FACES, EM_ARGS+2
.set EM_MATRIX, EM_ARGS+4
.set EM_POS, EM_ARGS+6
.set EM_CAMY, EM_ARGS+12
.set EM_VC, EM_ARGS+14
.set EM_FC, EM_ARGS+15
.set EM_SHIFT, EM_ARGS+16
.set EM_FLAGS, EM_ARGS+17
.set EM_OVERRIDE, EM_ARGS+18
.set EM_FCOLOR, EM_ARGS+19
.set EM_IDX, EM_ARGS+20
.set EM_ARGPTR, $93bc
.set EM_FACE, $93be
.set EM_I, $93bf
.set EM_REC, $93c0
.set EM_NEAR, $93c6
.set EM_VPTR, $93c7
.set EM_T, $93c9
.set EM_RESULT, $93cb
.globl fast_emit_mesh
.type fast_emit_mesh,@function
fast_emit_mesh:
    sta EM_FACE
    lda __rc2
    sta EM_ARGPTR
    lda __rc3
    sta EM_ARGPTR+1
    ldy #22
.Lem_copy_args:
    lda (__rc2),y
    sta EM_ARGS,y
    dey
    bpl .Lem_copy_args
    lda EM_FACE
    beq .Lem_transform
    jmp .Lem_faces
.Lem_transform:
    lda EM_VERT
    sta EM_VPTR
    lda EM_VERT+1
    sta EM_VPTR+1
    lda #0
    sta EM_I
.Lem_vertex:
    lda EM_I
    cmp EM_VC
    bcc .Lem_rotate
    jmp .Lem_faces
.Lem_rotate:
    lda EM_VPTR
    sta __rc2
    lda EM_VPTR+1
    sta __rc3
    lda EM_MATRIX
    sta __rc4
    lda EM_MATRIX+1
    sta __rc5
    lda EM_I
    jsr .Lem_record_address
    sta __rc6
    sty __rc7
    lda EM_SHIFT
    jsr fast_rotate_vertex
    ; record += position, each component saturating
    ldy #0
.Lem_translate:
    clc
    lda (__rc6),y
    adc EM_POS,y
    sta EM_T
    iny
    lda (__rc6),y
    adc EM_POS,y
    bvc .Lem_component_ok
    bmi .Lem_clamp_high
    lda #$00
    sta EM_T
    lda #$80
    bne .Lem_component_ok
.Lem_clamp_high:
    lda #$ff
    sta EM_T
    lda #$7f
.Lem_component_ok:
    sta (__rc6),y
    dey
    lda EM_T
    sta (__rc6),y
    iny
    iny
    cpy #6
    bne .Lem_translate
    ; y -= camera_y (wrapping), projected flag cleared
    ldy #2
    sec
    lda (__rc6),y
    sbc EM_CAMY
    sta (__rc6),y
    iny
    lda (__rc6),y
    sbc EM_CAMY+1
    sta (__rc6),y
    ldy #10
    lda #0
    sta (__rc6),y
    clc
    lda EM_VPTR
    adc #3
    sta EM_VPTR
    bcc .Lem_next_vertex
    inc EM_VPTR+1
.Lem_next_vertex:
    inc EM_I
    jmp .Lem_vertex
.Lem_faces:
    lda EM_FACE
    cmp EM_FC
    bcc .Lem_face_record
    jmp .Lem_finish
.Lem_face_record:
    ; face record = faces + 4 * face
    lda #0
    sta EM_T+1
    lda EM_FACE
    asl
    rol EM_T+1
    asl
    rol EM_T+1
    clc
    adc EM_FACES
    sta __rc2
    lda EM_T+1
    adc EM_FACES+1
    sta __rc3
    ldy #0
    lda (__rc2),y
    sta EM_IDX
    iny
    lda (__rc2),y
    sta EM_IDX+1
    iny
    lda (__rc2),y
    sta EM_IDX+2
    iny
    lda (__rc2),y
    ldx EM_OVERRIDE
    beq .Lem_face_color
    txa
.Lem_face_color:
    sta EM_FCOLOR
    ; record pointers and the count of corners at or behind the near plane
    lda #0
    sta EM_NEAR
    ldx #0
.Lem_corner_pointer:
    txa
    lsr
    tay
    lda EM_IDX,y
    jsr .Lem_record_address
    sta EM_REC,x
    sta __rc4
    tya
    sta EM_REC+1,x
    sta __rc5
    ldy #5
    lda (__rc4),y
    bmi .Lem_corner_next
    bne .Lem_corner_in
    dey
    lda (__rc4),y
    cmp #8
    bcc .Lem_corner_next
.Lem_corner_in:
    inc EM_NEAR
.Lem_corner_next:
    inx
    inx
    cpx #6
    bne .Lem_corner_pointer
    lda EM_NEAR
    beq .Lem_face_dropped
    cmp #3
    beq .Lem_face_visible
    jmp .Lem_clipped
.Lem_face_dropped:
    jmp .Lem_face_next
.Lem_face_visible:
    ; gather the three corners: project on first use, sum the depths
    lda #0
    sta ET_SUM
    sta ET_SUM+1
    sta ET_SUM+2
    ldx #0
.Lem_gather:
    lda EM_REC,x
    sta __rc2
    lda EM_REC+1,x
    sta __rc3
    ldy #10
    lda (__rc2),y
    bne .Lem_projected
    stx EM_T
    clc
    lda __rc2
    adc #6
    sta __rc4
    lda __rc3
    adc #0
    sta __rc5
    jsr fast_project
    ldx EM_T
    ldy #10
    lda #1
    sta (__rc2),y
.Lem_projected:
    ldy #6
    lda (__rc2),y
    sta ET_SX,x
    iny
    lda (__rc2),y
    sta ET_SX+1,x
    iny
    lda (__rc2),y
    sta ET_SY,x
    iny
    lda (__rc2),y
    sta ET_SY+1,x
    ldy #4
    clc
    lda (__rc2),y
    adc ET_SUM
    sta ET_SUM
    iny
    lda (__rc2),y
    adc ET_SUM+1
    sta ET_SUM+1
    bcc .Lem_gathered
    inc ET_SUM+2
.Lem_gathered:
    inx
    inx
    cpx #6
    bne .Lem_gather
    lda EM_FCOLOR
    sta ET_COLOR
    lda EM_FLAGS
    sta ET_FLAGS
    jsr .Let_emit
.Lem_face_next:
    inc EM_FACE
    jmp .Lem_faces
.Lem_clipped:
    lda EM_FACE
    jmp .Lem_return
.Lem_finish:
    lda EM_FC
.Lem_return:
    sta EM_RESULT
    ; write back the face handed to the caller
    lda EM_ARGPTR
    sta __rc2
    lda EM_ARGPTR+1
    sta __rc3
    ldy #19
.Lem_write_face:
    lda EM_ARGS,y
    sta (__rc2),y
    iny
    cpy #23
    bne .Lem_write_face
    lda EM_RESULT
    rts
; A = vertex index (< 128) -> record address $E000 + 16 * index: low byte in
; A, high byte in Y; X is preserved.
.Lem_record_address:
    sta EM_T
    lsr
    lsr
    lsr
    lsr
    clc
    adc #>RECORDS_BASE
    tay
    lda EM_T
    asl
    asl
    asl
    asl
    rts
.size fast_emit_mesh, .-fast_emit_mesh
