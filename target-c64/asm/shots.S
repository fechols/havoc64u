; uint8_t fast_shot_scan(int16_t old_z, const Vec3 *pos, const Actor *actors,
;                        const uint8_t *specs, uint8_t enemy_count)
; C ABI: old_z A/X, pos rc2/3, actors rc4/5, specs rc6/7, enemy_count rc8;
; result A = the first actor slot the shot hits, or $FF.
; The native body of the actor scan in Game::update_shots (Game::shot_target)
; for one player shot or missile that has just moved from depth old_z to pos:
; actors is the twelve 16-byte actor slots, specs the bank's enemy descriptors
; (8 bytes each, kinds 1..enemy_count).
; In slot order, a slot with kind 0 is skipped. Otherwise its radius is 18 for
; small debris (flag bit 1), 35 for a BUILDING (6) or ROCK (7), else the
; descriptor's shot_radius (0 for a kind past enemy_count); the slot is hit
; when |x - actor.x| <= radius, the depth sweep [min(old_z, z), max(old_z, z)]
; meets [actor.z - radius, actor.z + radius] (fast_swept_z: wrapping bounds,
; signed comparisons) and |y - (actor.y + center)| <= radius, with center 10
; or 20 for a small or ordinary ROCK, 30 for a BUILDING, else the descriptor's
; center_y (0 past enemy_count). Differences wrap and i16::abs(-32768) stays
; negative, so it lies within every radius, as in the release build's
; close_xy. The lateral test runs first: it needs no radius while the
; difference does not fit a byte, which rejects most slots. Only caller-saved
; rc2..19 and A/X/Y change; no scratch RAM or stack frame.
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
.set SS_T, __rc2                ; 16-bit temporary once the position is copied
.set SS_ACT, __rc4              ; the slot under test (the actors argument)
.set SS_SPEC, __rc6
.set SS_EC, __rc8
.set SS_F, __rc9                ; small debris flag, then the centre offset
.set SS_LO, __rc10              ; min(old_z, z)
.set SS_HI, __rc12              ; max(old_z, z)
.set SS_X, __rc14
.set SS_Y, __rc16
.set SS_R, __rc18               ; the slot's radius
.set SS_K, __rc19               ; the slot's kind

.section .phase.sim.shots,"ax",@progbits
.globl fast_shot_scan
.type fast_shot_scan,@function
fast_shot_scan:
    sta SS_LO
    stx SS_LO+1
    ldy #0
    lda (__rc2),y
    sta SS_X
    iny
    lda (__rc2),y
    sta SS_X+1
    iny
    lda (__rc2),y
    sta SS_Y
    iny
    lda (__rc2),y
    sta SS_Y+1
    iny
    lda (__rc2),y
    sta SS_HI
    iny
    lda (__rc2),y
    sta SS_HI+1
    ; order the sweep: SS_LO = min(old_z, z), SS_HI = max(old_z, z), signed
    sec
    lda SS_HI
    sbc SS_LO
    lda SS_HI+1
    sbc SS_LO+1
    bvc .Lss_order_sign
    eor #$80
.Lss_order_sign:
    bpl .Lss_ordered
    ldx SS_LO
    lda SS_HI
    sta SS_LO
    stx SS_HI
    ldx SS_LO+1
    lda SS_HI+1
    sta SS_LO+1
    stx SS_HI+1
.Lss_ordered:
    ldx #0
.Lss_actor:
    ldy #12
    lda (SS_ACT),y              ; kind
    beq .Lss_next
    sta SS_K
    ; |x - actor.x| must fit a byte: every radius does
    ldy #0
    sec
    lda SS_X
    sbc (SS_ACT),y
    sta SS_T
    iny
    lda SS_X+1
    sbc (SS_ACT),y
    bpl .Lss_dx_magnitude
    sta SS_T+1
    sec
    lda #0
    sbc SS_T
    sta SS_T
    lda #0
    sbc SS_T+1
.Lss_dx_magnitude:
    beq .Lss_dx_near
    bpl .Lss_next               ; 256 or more
    lda #0                      ; -32768 stays negative: within every radius
    sta SS_T
.Lss_dx_near:
    ldy #14
    lda (SS_ACT),y              ; flags
    and #2
    sta SS_F
    beq .Lss_kind_radius
    lda #18                     ; small debris
    bne .Lss_radius
.Lss_kind_radius:
    lda SS_K
    cmp #6
    beq .Lss_fixed_radius
    cmp #7
    beq .Lss_fixed_radius
    cmp SS_EC
    beq .Lss_spec_radius
    bcs .Lss_default_radius     ; a kind past the bank's descriptors
.Lss_spec_radius:
    sec
    sbc #1
    asl
    asl
    asl
    tay
    iny
    iny
    lda (SS_SPEC),y             ; shot_radius
    jmp .Lss_radius
.Lss_default_radius:
    lda #0
    beq .Lss_radius
.Lss_fixed_radius:
    lda #35                     ; BUILDING or ROCK
.Lss_radius:
    sta SS_R
    cmp SS_T
    bcs .Lss_depth              ; |dx| <= radius
.Lss_next:
    clc
    lda SS_ACT
    adc #16
    sta SS_ACT
    bcc .Lss_count
    inc SS_ACT+1
.Lss_count:
    inx
    cpx #12
    beq .Lss_miss
    jmp .Lss_actor
.Lss_miss:
    lda #$ff
    rts
.Lss_depth:
    ; actor.z + radius >= lo
    ldy #4
    clc
    lda (SS_ACT),y
    adc SS_R
    sta SS_T
    iny
    lda (SS_ACT),y
    adc #0
    sta SS_T+1
    sec
    lda SS_T
    sbc SS_LO
    lda SS_T+1
    sbc SS_LO+1
    bvc .Lss_upper_sign
    eor #$80
.Lss_upper_sign:
    bmi .Lss_next               ; the slot ends before the sweep starts
    ; actor.z - radius <= hi
    ldy #4
    sec
    lda (SS_ACT),y
    sbc SS_R
    sta SS_T
    iny
    lda (SS_ACT),y
    sbc #0
    sta SS_T+1
    sec
    lda SS_HI
    sbc SS_T
    lda SS_HI+1
    sbc SS_T+1
    bvc .Lss_lower_sign
    eor #$80
.Lss_lower_sign:
    bmi .Lss_next               ; the slot starts after the sweep ends
    ; the centre offset
    lda SS_K
    cmp #7
    beq .Lss_rock_center
    cmp #6
    beq .Lss_building_center
    cmp SS_EC
    beq .Lss_spec_center
    bcs .Lss_zero_center
.Lss_spec_center:
    sec
    sbc #1
    asl
    asl
    asl
    tay
    iny
    lda (SS_SPEC),y             ; center_y (i8)
    jmp .Lss_center
.Lss_rock_center:
    lda SS_F
    beq .Lss_large_rock
    lda #10
    bne .Lss_center
.Lss_large_rock:
    lda #20
    bne .Lss_center
.Lss_building_center:
    lda #30
    bne .Lss_center
.Lss_zero_center:
    lda #0
.Lss_center:
    sta SS_F
    ; dy = y - actor.y - centre, wrapping
    ldy #2
    sec
    lda SS_Y
    sbc (SS_ACT),y
    sta SS_T
    iny
    lda SS_Y+1
    sbc (SS_ACT),y
    sta SS_T+1
    sec
    lda SS_T
    sbc SS_F
    sta SS_T
    lda SS_F
    bmi .Lss_center_negative
    lda SS_T+1
    sbc #0
    jmp .Lss_dy_sign
.Lss_center_negative:
    lda SS_T+1
    sbc #$ff
.Lss_dy_sign:
    bpl .Lss_dy_magnitude
    sta SS_T+1
    sec
    lda #0
    sbc SS_T
    sta SS_T
    lda #0
    sbc SS_T+1
.Lss_dy_magnitude:
    bmi .Lss_hit                ; -32768 stays negative: within every radius
    bne .Lss_next_far
    lda SS_R
    cmp SS_T
    bcs .Lss_hit                ; |dy| <= radius
.Lss_next_far:
    jmp .Lss_next
.Lss_hit:
    txa
    rts
.size fast_shot_scan, .-fast_shot_scan
