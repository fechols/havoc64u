; Exact Q8 product floor((a * b) / 256) truncated to 16 bits: the native body
; of havoc::math::mul_q8. C ABI: a in A/X, b in rc2/3, result in A/X.
; The magnitudes multiply through unsigned 8x8 partial products taken from the
; quarter-square table BOOT builds at $C000 (Q[n] = floor(n*n/4), n = 0..511,
; 512 low bytes then 512 high bytes): x*y = Q[x+y] - Q[|x-y|]. The 24 low
; product bits are enough: the result keeps bits 8..23, and a negative product
; rounds toward minus infinity exactly as the arithmetic shift does.
; Only caller-saved rc2..13 and A/X/Y change; no scratch RAM or stack frame.
; Resident in the $9900 table page with the sort kernel, keeping $0801-$2FFF
; for compiled code.
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
.set QLO, $c000
.set QHI, $c200

.section .tables.fast_mul_q8,"ax",@progbits
.globl fast_mul_q8
.type fast_mul_q8,@function
fast_mul_q8:
    sta __rc4
    stx __rc5
; Product core: |a| in rc4/5 and |b| in rc2/3 after sign handling; A/X result.
.Lmq_product:
    lda __rc5
    eor __rc3
    sta __rc8
    lda __rc5
    bpl .Lmq_a_positive
    sec
    lda #0
    sbc __rc4
    sta __rc4
    lda #0
    sbc __rc5
    sta __rc5
.Lmq_a_positive:
    lda __rc3
    bpl .Lmq_b_positive
    sec
    lda #0
    sbc __rc2
    sta __rc2
    lda #0
    sbc __rc3
    sta __rc3
.Lmq_b_positive:
    ; m = al*bl + (ah*bl + al*bh) << 8 + (ah*bh) << 16, kept in rc6/7/9.
    ldx __rc4
    ldy __rc2
    jsr .Lmq_mul8
    sta __rc6
    stx __rc7
    lda #0
    sta __rc9
    ldx __rc5
    beq .Lmq_high_multiplier
    ldy __rc2
    jsr .Lmq_mul8
    clc
    adc __rc7
    sta __rc7
    txa
    adc __rc9
    sta __rc9
.Lmq_high_multiplier:
    ldy __rc3
    beq .Lmq_combine
    ldx __rc4
    jsr .Lmq_mul8
    clc
    adc __rc7
    sta __rc7
    txa
    adc __rc9
    sta __rc9
    ldx __rc5
    beq .Lmq_combine
    ldy __rc3
    jsr .Lmq_mul8
    clc
    adc __rc9
    sta __rc9
.Lmq_combine:
    lda __rc8
    bpl .Lmq_positive
    ; -(ceil(m / 256)): add one when any low bit is set, then negate.
    lda __rc6
    beq .Lmq_negate
    inc __rc7
    bne .Lmq_negate
    inc __rc9
.Lmq_negate:
    sec
    lda #0
    sbc __rc7
    tay
    lda #0
    sbc __rc9
    tax
    tya
    rts
.Lmq_positive:
    lda __rc7
    ldx __rc9
    rts
; Unsigned 8x8 product: X = x, Y = y; result low byte in A, high byte in X.
; Also exported as fast_mul8 for the projection kernel; clobbers rc10-13 only.
.globl fast_mul8
fast_mul8:
.Lmq_mul8:
    stx __rc10
    sty __rc11
    txa
    clc
    adc __rc11
    tax
    bcs .Lmq_sum_high
    lda QLO,x
    sta __rc12
    lda QHI,x
    sta __rc13
    jmp .Lmq_difference
.Lmq_sum_high:
    lda QLO+256,x
    sta __rc12
    lda QHI+256,x
    sta __rc13
.Lmq_difference:
    lda __rc10
    sec
    sbc __rc11
    bcs .Lmq_magnitude
    eor #$ff
    clc
    adc #1
.Lmq_magnitude:
    tax
    sec
    lda __rc12
    sbc QLO,x
    tay
    lda __rc13
    sbc QHI,x
    tax
    tya
    rts
.size fast_mul_q8, .-fast_mul_q8

; void fast_mesh_matrix(const int16_t sincos[6], int16_t out[9])
; C ABI: sincos rc2/3 (sy, cy, sp, cp, sr, cr), out rc4/5. The native body of
; RotationMatrix::compose: M = R_roll * R_pitch * R_yaw from the game's pair
; rotations, every product rounded by the resident Q8 core in the table order
; below, then each entry clamped to [-256, 256]. Operands live in 16-bit slots
; at $9100: the six inputs, the constant 256, two temporaries and the nine
; entries. Both mesh kernels live in GEOMETRY, the only phase that rotates
; vertices; the product core they call stays resident. The IRQ never touches
; this scratch.
.set RM_SLOTS, $9100
.set RM_OUT, $9124
.set RM_INDEX, $9126
.set RM_PROD, $9127
.section .phase.geometry,"ax",@progbits
.globl fast_mesh_matrix
.type fast_mesh_matrix,@function
fast_mesh_matrix:
    lda __rc4
    sta RM_OUT
    lda __rc5
    sta RM_OUT+1
    ldy #11
.Lrm_copy:
    lda (__rc2),y
    sta RM_SLOTS,y
    dey
    bpl .Lrm_copy
    lda #0
    sta RM_SLOTS+12
    sta RM_INDEX
    lda #1
    sta RM_SLOTS+13
.Lrm_step:
    ldx RM_INDEX
    cpx #60
    bcc .Lrm_product
    jmp .Lrm_clamp
.Lrm_product:
    ldy .Lrm_table,x
    lda RM_SLOTS,y
    sta __rc4
    lda RM_SLOTS+1,y
    sta __rc5
    ldy .Lrm_table+1,x
    lda RM_SLOTS,y
    sta __rc2
    lda RM_SLOTS+1,y
    sta __rc3
    jsr .Lmq_product
    sta RM_PROD
    stx RM_PROD+1
    ldx RM_INDEX
    ldy .Lrm_table+2,x
    lda .Lrm_table+3,x
    beq .Lrm_store
    cmp #1
    beq .Lrm_store_negated
    cmp #2
    beq .Lrm_add
    sec
    lda RM_SLOTS,y
    sbc RM_PROD
    sta RM_SLOTS,y
    lda RM_SLOTS+1,y
    sbc RM_PROD+1
    sta RM_SLOTS+1,y
    jmp .Lrm_advance
.Lrm_add:
    clc
    lda RM_SLOTS,y
    adc RM_PROD
    sta RM_SLOTS,y
    lda RM_SLOTS+1,y
    adc RM_PROD+1
    sta RM_SLOTS+1,y
    jmp .Lrm_advance
.Lrm_store_negated:
    sec
    lda #0
    sbc RM_PROD
    sta RM_SLOTS,y
    lda #0
    sbc RM_PROD+1
    sta RM_SLOTS+1,y
    jmp .Lrm_advance
.Lrm_store:
    lda RM_PROD
    sta RM_SLOTS,y
    lda RM_PROD+1
    sta RM_SLOTS+1,y
.Lrm_advance:
    lda RM_INDEX
    clc
    adc #4
    sta RM_INDEX
    jmp .Lrm_step
.Lrm_clamp:
    lda RM_OUT
    sta __rc4
    lda RM_OUT+1
    sta __rc5
    ldy #0
.Lrm_clamp_loop:
    lda RM_SLOTS+19,y
    bmi .Lrm_clamp_negative
    beq .Lrm_clamp_copy
    cmp #1
    bne .Lrm_clamp_positive
    lda RM_SLOTS+18,y
    beq .Lrm_clamp_copy
.Lrm_clamp_positive:
    lda #0
    sta RM_SLOTS+18,y
    lda #1
    sta RM_SLOTS+19,y
    jmp .Lrm_clamp_copy
.Lrm_clamp_negative:
    cmp #$ff
    beq .Lrm_clamp_copy
    lda #0
    sta RM_SLOTS+18,y
    lda #$ff
    sta RM_SLOTS+19,y
.Lrm_clamp_copy:
    lda RM_SLOTS+18,y
    sta (__rc4),y
    iny
    lda RM_SLOTS+18,y
    sta (__rc4),y
    iny
    cpy #18
    bne .Lrm_clamp_loop
    rts
; Product table: operand A slot, operand B slot, destination slot, operation
; (0 store, 1 store negated, 2 add, 3 subtract). Slots: sy 0, cy 2, sp 4,
; cp 6, sr 8, cr 10, one 12, t1 14, t2 16, m00 18 ... m22 34.
.Lrm_table:
    .byte 4, 0, 14, 0
    .byte 4, 2, 16, 0
    .byte 10, 2, 18, 0
    .byte 8, 14, 18, 3
    .byte 8, 6, 20, 1
    .byte 10, 0, 22, 0
    .byte 8, 16, 22, 2
    .byte 8, 2, 24, 0
    .byte 10, 14, 24, 2
    .byte 10, 6, 26, 0
    .byte 8, 0, 28, 0
    .byte 10, 16, 28, 3
    .byte 6, 0, 30, 1
    .byte 4, 12, 32, 0
    .byte 6, 2, 34, 0
.size fast_mesh_matrix, .-fast_mesh_matrix

; void fast_rotate_vertex(const int8_t v[3], const int16_t m[9], Vec3 *out, uint8_t shift)
; C ABI: v rc2/3, m rc4/5 (row-major Q8, entries within +-256), out rc6/7,
; shift in A (4..9). The native body of RotationMatrix::apply:
; out[i] = (sum_j m[i][j] * v[j]) >> shift with the arithmetic shift's floor.
; Each product is |m| * |v| from the quarter-square core (|m| = 256 is
; |v| << 8), added to or subtracted from a 24-bit accumulator by the XOR of the
; two signs. The shift takes the accumulator's upper two bytes and ORs in the
; shifted low byte, so only 8 - shift single-bit shifts run per row.
.set RV_SHIFT, $9130
.set RV_AV, $9131
.set RV_SV, $9134
.set RV_ACC, $9137
.set RV_MIDX, $913a
.set RV_J, $913b
.set RV_ROW, $913c
.set RV_AM, $913d
.set RV_NEG, $913f
.section .phase.geometry,"ax",@progbits
.globl fast_rotate_vertex
.type fast_rotate_vertex,@function
fast_rotate_vertex:
    sta RV_SHIFT
    ldy #0
.Lrv_abs:
    lda (__rc2),y
    sta RV_SV,y
    bpl .Lrv_abs_ok
    eor #$ff
    clc
    adc #1
.Lrv_abs_ok:
    sta RV_AV,y
    iny
    cpy #3
    bne .Lrv_abs
    lda #0
    sta RV_MIDX
    sta RV_ROW
.Lrv_row:
    lda #0
    sta RV_ACC
    sta RV_ACC+1
    sta RV_ACC+2
    sta RV_J
.Lrv_col:
    ldy RV_MIDX
    lda (__rc4),y
    sta RV_AM
    iny
    lda (__rc4),y
    sta RV_AM+1
    iny
    sty RV_MIDX
    ldx RV_J
    eor RV_SV,x
    sta RV_NEG
    ; Zero entries (most of any single-axis or identity matrix) add nothing.
    lda RV_AM
    ora RV_AM+1
    beq .Lrv_next
    lda RV_AM+1
    bpl .Lrv_m_magnitude
    sec
    lda #0
    sbc RV_AM
    sta RV_AM
    lda #0
    sbc RV_AM+1
    sta RV_AM+1
.Lrv_m_magnitude:
    lda RV_AM+1
    beq .Lrv_small_m
    ; |m| = 256: the product is |v| << 8.
    lda RV_AV,x
    tax
    lda #0
    beq .Lrv_accumulate
.Lrv_small_m:
    ldy RV_AV,x
    ldx RV_AM
    jsr .Lmq_mul8
.Lrv_accumulate:
    bit RV_NEG
    bmi .Lrv_subtract
    clc
    adc RV_ACC
    sta RV_ACC
    txa
    adc RV_ACC+1
    sta RV_ACC+1
    bcc .Lrv_next
    inc RV_ACC+2
    jmp .Lrv_next
.Lrv_subtract:
    sec
    eor #$ff
    adc RV_ACC
    sta RV_ACC
    txa
    eor #$ff
    adc RV_ACC+1
    sta RV_ACC+1
    bcs .Lrv_next
    dec RV_ACC+2
.Lrv_next:
    inc RV_J
    lda RV_J
    cmp #3
    beq .Lrv_row_done
    jmp .Lrv_col
.Lrv_row_done:
    lda RV_SHIFT
    cmp #9
    bcc .Lrv_shift_small
    ; shift 9: the upper bytes shifted right once, arithmetically.
    lda RV_ACC+2
    cmp #$80
    ror RV_ACC+2
    ror RV_ACC+1
    jmp .Lrv_store
.Lrv_shift_small:
    sec
    lda #8
    sbc RV_SHIFT
    beq .Lrv_store
    tax
.Lrv_shift_left:
    asl RV_ACC+1
    rol RV_ACC+2
    dex
    bne .Lrv_shift_left
    ldx RV_SHIFT
    lda RV_ACC
.Lrv_shift_right:
    lsr
    dex
    bne .Lrv_shift_right
    ora RV_ACC+1
    sta RV_ACC+1
.Lrv_store:
    ldy RV_ROW
    lda RV_ACC+1
    sta (__rc6),y
    iny
    lda RV_ACC+2
    sta (__rc6),y
    iny
    sty RV_ROW
    cpy #6
    beq .Lrv_done
    jmp .Lrv_row
.Lrv_done:
    rts
.size fast_rotate_vertex, .-fast_rotate_vertex