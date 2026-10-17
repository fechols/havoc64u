; Native multicolor cell quantization, byte-for-byte equivalent to the Rust
; reference. Six pointer arguments use rc2/3, rc4/5, ..., rc12/13 respectively:
; pixels, previous screen, previous colors, bitmap, screen, colors.
; rc14..19 are volatile; rc20..31 and the software stack are untouched.
; $77A0-$77F8 is private main-thread scratch, never used by the audio IRQ.

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

.set PK_COUNT, $77a0
.set PK_HUD, $77b0
.set PK_FLAGS, $77c0
.set PK_MAP, $77d0
.set PK_PAL, $77e0
.set PK_OLD, $77e4
.set PK_CHOSEN, $77e7
.set PK_CELL, $77ea
.set PK_ROW, $77eb
.set PK_UNIFORM, $77ec
.set PK_NONBLACK, $77ed
.set PK_RAW, $77ee
.set PK_PICK, $77ef
.set PK_BEST, $77f0
.set PK_BEST_LO, $77f1
.set PK_BEST_HI, $77f2
.set PK_SCORE, $77f3
.set PK_COLOR, $77f4
.set PK_SLOT, $77f5
.set PK_DIST_LO, $77f6
.set PK_DIST_HI, $77f7
.set PK_PACKED, $77f8

.section .phase.pixels,"ax",@progbits
.globl fast_pack_band
fast_pack_band:
    lda #40
    sta PK_CELL
.Lpk_cell:
    lda #0
    ldx #3
.Lpk_clear_palette:
    sta PK_PAL,x
    dex
    bpl .Lpk_clear_palette
    ldy #0
    lda (__rc2),y
    and #15
    sta PK_UNIFORM
    lda (__rc4),y
    pha
    lsr
    lsr
    lsr
    lsr
    sta PK_OLD
    pla
    and #15
    sta PK_OLD+1
    lda (__rc6),y
    and #15
    sta PK_OLD+2
    lda __rc2
    sta __rc14
    lda __rc3
    sta __rc15
    lda #8
    sta PK_ROW
.Lpk_uniform_row:
    ldy #0
    lda (__rc14),y
    and #15
    cmp PK_UNIFORM
    bne .Lpk_mixed
    iny
    lda (__rc14),y
    and #15
    cmp PK_UNIFORM
    bne .Lpk_mixed
    iny
    lda (__rc14),y
    and #15
    cmp PK_UNIFORM
    bne .Lpk_mixed
    iny
    lda (__rc14),y
    and #15
    cmp PK_UNIFORM
    bne .Lpk_mixed
    clc
    lda __rc14
    adc #160
    sta __rc14
    bcc .Lpk_uniform_next
    inc __rc15
.Lpk_uniform_next:
    dec PK_ROW
    bne .Lpk_uniform_row

    ; A single-color cell needs neither ranking nor nearest-color lookup.
    ldx #0
    lda PK_UNIFORM
    beq .Lpk_uniform_slot
.Lpk_uniform_find:
    cmp PK_OLD,x
    beq .Lpk_uniform_found
    inx
    cpx #3
    bne .Lpk_uniform_find
    ldx #0
.Lpk_uniform_found:
    inx
.Lpk_uniform_slot:
    sta PK_PAL,x
    lda .Lpk_patterns,x
    ldy #7
.Lpk_uniform_write:
    sta (__rc8),y
    dey
    bpl .Lpk_uniform_write
    jmp .Lpk_attributes

.Lpk_mixed:
    ; Only cells with multiple colors need a histogram or HUD priorities.
    lda #0
    ldx #15
.Lpk_clear_counts:
    sta PK_COUNT,x
    sta PK_HUD,x
    dex
    bpl .Lpk_clear_counts
    lda __rc2
    sta __rc14
    lda __rc3
    sta __rc15
    lda #8
    sta PK_ROW
.Lpk_histogram_row:
    ldy #0
.Lpk_histogram_pixel:
    lda (__rc14),y
    bpl .Lpk_histogram_not_hud
    and #15
    tax
    inc PK_COUNT,x
    lda #1
    sta PK_HUD,x
    jmp .Lpk_histogram_advance
.Lpk_histogram_not_hud:
    and #15
    tax
    inc PK_COUNT,x
.Lpk_histogram_advance:
    iny
    cpy #4
    bne .Lpk_histogram_pixel
    clc
    lda __rc14
    adc #160
    sta __rc14
    bcc .Lpk_histogram_next
    inc __rc15
.Lpk_histogram_next:
    dec PK_ROW
    bne .Lpk_histogram_row

.Lpk_select_begin:
    lda #0
    sta PK_NONBLACK
    ldx #15
.Lpk_clear_flags:
    sta PK_FLAGS,x
    sta PK_MAP,x
    dex
    bpl .Lpk_clear_flags
    sta PK_CHOSEN
    sta PK_CHOSEN+1
    sta PK_CHOSEN+2
    ; A stable top-three insertion pass ranks each present color once.
    ; MAP[0..3] temporarily holds score lows, MAP[3..6] the HUD priorities.
    ; Ascending color iteration and strict comparison retain Rust's tie order.
    ldx #1
.Lpk_rank_color:
    lda PK_COUNT,x
    bne .Lpk_rank_present
    jmp .Lpk_rank_next
.Lpk_rank_present:
    inc PK_NONBLACK
    asl
    asl
    sta PK_SCORE
    txa
    cmp PK_OLD
    beq .Lpk_rank_retained
    cmp PK_OLD+1
    beq .Lpk_rank_retained
    cmp PK_OLD+2
    bne .Lpk_rank_compare
.Lpk_rank_retained:
    inc PK_SCORE
.Lpk_rank_compare:
    ldy #0
.Lpk_rank_slot:
    lda PK_HUD,x
    cmp PK_MAP+3,y
    bcc .Lpk_rank_slot_next
    bne .Lpk_rank_best
    lda PK_SCORE
    cmp PK_MAP,y
    bcc .Lpk_rank_slot_next
    bne .Lpk_rank_best
.Lpk_rank_slot_next:
    iny
    cpy #3
    bne .Lpk_rank_slot
    jmp .Lpk_rank_next
.Lpk_rank_best:
    cpy #2
    beq .Lpk_rank_insert
    lda PK_CHOSEN+1
    sta PK_CHOSEN+2
    lda PK_MAP+1
    sta PK_MAP+2
    lda PK_MAP+4
    sta PK_MAP+5
    cpy #1
    beq .Lpk_rank_insert
    lda PK_CHOSEN
    sta PK_CHOSEN+1
    lda PK_MAP
    sta PK_MAP+1
    lda PK_MAP+3
    sta PK_MAP+4
.Lpk_rank_insert:
    txa
    sta PK_CHOSEN,y
    lda PK_HUD,x
    sta PK_MAP+3,y
    lda PK_SCORE
    sta PK_MAP,y
.Lpk_rank_next:
    inx
    cpx #16
    beq .Lpk_rank_done
    jmp .Lpk_rank_color
.Lpk_rank_done:
    ldx PK_CHOSEN
    lda #1
    sta PK_FLAGS,x
    ldx PK_CHOSEN+1
    sta PK_FLAGS,x
    ldx PK_CHOSEN+2
    sta PK_FLAGS,x
    lda #0
    ldx #5
.Lpk_clear_scores:
    sta PK_MAP,x
    dex
    bpl .Lpk_clear_scores

    ; Keep retained colors in their preceding-frame hardware slots.
    ldx #0
.Lpk_retain_slot:
    lda PK_OLD,x
    beq .Lpk_retain_next
    tay
    lda PK_FLAGS,y
    cmp #1
    bne .Lpk_retain_next
    tya
    sta PK_PAL+1,x
    lda #3
    sta PK_FLAGS,y
.Lpk_retain_next:
    inx
    cpx #3
    bne .Lpk_retain_slot
    lda #0
    sta PK_PICK
.Lpk_assign_color:
    ldx PK_PICK
    lda PK_CHOSEN,x
    beq .Lpk_assign_next
    sta PK_COLOR
    tay
    lda PK_FLAGS,y
    and #2
    bne .Lpk_assign_next
    ldx #1
.Lpk_find_empty:
    lda PK_PAL,x
    beq .Lpk_assign_empty
    inx
    cpx #4
    bne .Lpk_find_empty
    jmp .Lpk_assign_next
.Lpk_assign_empty:
    lda PK_COLOR
    sta PK_PAL,x
    tay
    lda #3
    sta PK_FLAGS,y
.Lpk_assign_next:
    inc PK_PICK
    lda PK_PICK
    cmp #3
    bne .Lpk_assign_color

    ; Every retained/chosen color has exact distance zero. Assign those
    ; hardware slots directly; only omitted colors need distance comparisons.
    ; This also handles every mixed cell with <=3 nonblack colors exactly.
    ldx #1
.Lpk_exact_slot:
    lda PK_PAL,x
    beq .Lpk_exact_next
    tay
    txa
    sta PK_MAP,y
.Lpk_exact_next:
    inx
    cpx #4
    bne .Lpk_exact_slot

    ; With at most three nonblack colors, exact-slot assignment covered every
    ; present color. Ranking/retention still ran, preserving all palette bytes.
    lda PK_NONBLACK
    cmp #4
    bcs .Lpk_omitted_colors
    jmp .Lpk_map_done
.Lpk_omitted_colors:

    ; COLOR_DISTANCE is a 16x16 little-endian u16 table shared with Rust.
    ldx #1
.Lpk_map_color:
    lda PK_FLAGS,x
    bne .Lpk_map_next
    lda PK_COUNT,x
    beq .Lpk_map_next
    stx PK_COLOR
    txa
    lsr
    lsr
    lsr
    clc
    adc #>COLOR_DISTANCE
    sta __rc19
    txa
    asl
    asl
    asl
    asl
    asl
    clc
    adc #<COLOR_DISTANCE
    sta __rc18
    bcc .Lpk_distance_address
    inc __rc19
.Lpk_distance_address:
    ldy #0
    lda (__rc18),y
    sta PK_BEST_LO
    iny
    lda (__rc18),y
    sta PK_BEST_HI
    lda #1
    sta PK_SLOT
.Lpk_map_slot:
    ldx PK_SLOT
    lda PK_PAL,x
    asl
    tay
    lda (__rc18),y
    sta PK_DIST_LO
    iny
    lda (__rc18),y
    sta PK_DIST_HI
    cmp PK_BEST_HI
    bcc .Lpk_map_best
    bne .Lpk_map_slot_next
    lda PK_DIST_LO
    cmp PK_BEST_LO
    bcs .Lpk_map_slot_next
.Lpk_map_best:
    lda PK_DIST_LO
    sta PK_BEST_LO
    lda PK_DIST_HI
    sta PK_BEST_HI
    lda PK_SLOT
    ldx PK_COLOR
    sta PK_MAP,x
.Lpk_map_slot_next:
    inc PK_SLOT
    lda PK_SLOT
    cmp #4
    bne .Lpk_map_slot
    ldx PK_COLOR
.Lpk_map_next:
    inx
    cpx #16
    beq .Lpk_map_done
    jmp .Lpk_map_color
.Lpk_map_done:

    lda __rc2
    sta __rc14
    lda __rc3
    sta __rc15
    lda #0
    sta PK_ROW
.Lpk_bitmap_row:
    ; Four fixed pixels: keep the shifting accumulator in A rather than
    ; performing two slow read-modify-writes for every packed pixel.
    ldy #0
    lda (__rc14),y
    and #15
    tax
    lda PK_MAP,x
    asl
    asl
    sta PK_PACKED
    iny
    lda (__rc14),y
    and #15
    tax
    lda PK_MAP,x
    ora PK_PACKED
    asl
    asl
    sta PK_PACKED
    iny
    lda (__rc14),y
    and #15
    tax
    lda PK_MAP,x
    ora PK_PACKED
    asl
    asl
    sta PK_PACKED
    iny
    lda (__rc14),y
    and #15
    tax
    lda PK_MAP,x
    ora PK_PACKED
    ldy PK_ROW
    sta (__rc8),y
    clc
    lda __rc14
    adc #160
    sta __rc14
    bcc .Lpk_bitmap_next
    inc __rc15
.Lpk_bitmap_next:
    inc PK_ROW
    lda PK_ROW
    cmp #8
    bne .Lpk_bitmap_row

.Lpk_attributes:
    lda PK_PAL+1
    asl
    asl
    asl
    asl
    ora PK_PAL+2
    ldy #0
    sta (__rc10),y
    lda PK_PAL+3
    sta (__rc12),y
    clc
    lda __rc2
    adc #4
    sta __rc2
    bcc .Lpk_next_pixels
    inc __rc3
.Lpk_next_pixels:
    inc __rc4
    bne .Lpk_next_previous_screen
    inc __rc5
.Lpk_next_previous_screen:
    inc __rc6
    bne .Lpk_next_previous_color
    inc __rc7
.Lpk_next_previous_color:
    clc
    lda __rc8
    adc #8
    sta __rc8
    bcc .Lpk_next_bitmap
    inc __rc9
.Lpk_next_bitmap:
    inc __rc10
    bne .Lpk_next_screen
    inc __rc11
.Lpk_next_screen:
    inc __rc12
    bne .Lpk_next_color
    inc __rc13
.Lpk_next_color:
    dec PK_CELL
    beq .Lpk_done
    jmp .Lpk_cell
.Lpk_done:
    rts
.Lpk_patterns:
    .byte $00,$55,$aa,$ff
