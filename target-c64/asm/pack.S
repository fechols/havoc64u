; Native multicolor cell quantization, byte-for-byte equivalent to the Rust
; reference render::pack_cell: the fallback of the cell-direct rasterizer for a
; cell whose pixel form holds more colours than the three hardware slots.
; Code 0 is the VIC background, which shows the frame's clear colour (the
; cell writer keeps it in zero page $FF): that colour needs no slot and is
; the nearest-colour fallback of every omitted colour; black is an ordinary
; colour. Unused slots hold the background colour, which also marks "no
; colour" in the working tables below, since no slot ever holds it.
; C ABI: cell A, bitmap rc2/3 (already at the cell's eight bytes), screen
; rc4/5 and colors rc6/7 (band bases). rc2..13 are clobbered; rc14..19 are
; volatile; rc20..31 and the software stack are untouched.
; $77A0-$77FA is private main-thread scratch, never used by the audio IRQ.
; The pixel band is the fixed workspace at $8C00 (rows 160 bytes apart).
; In-place contract: the cell's previous screen byte and color nibble are read
; at .Lpk_cell before any of the cell's outputs are written at .Lpk_attributes.
; The previous-screen and previous-color pointers may therefore equal the
; outputs, including live color RAM, whose undefined upper nibble is masked.

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

; Packer scratch (the mos-sim harness relocates the base).
.ifndef PK_SCRATCH
.set PK_SCRATCH, $77a0
.endif
.set PK_COUNT, PK_SCRATCH+0
.set PK_HUD, PK_SCRATCH+16
.set PK_FLAGS, PK_SCRATCH+32
.set PK_MAP, PK_SCRATCH+48
.set PK_PAL, PK_SCRATCH+64
.set PK_OLD, PK_SCRATCH+68
.set PK_CHOSEN, PK_SCRATCH+71
.set PK_ROW, PK_SCRATCH+75
.set PK_UNIFORM, PK_SCRATCH+76
.set PK_NONBLACK, PK_SCRATCH+77
.set PK_PICK, PK_SCRATCH+79
.set PK_BEST_LO, PK_SCRATCH+81
.set PK_BEST_HI, PK_SCRATCH+82
.set PK_SCORE, PK_SCRATCH+83
.set PK_COLOR, PK_SCRATCH+84
.set PK_SLOT, PK_SCRATCH+85
.set PK_DIST_LO, PK_SCRATCH+86
.set PK_DIST_HI, PK_SCRATCH+87
.set PK_PACKED, PK_SCRATCH+88
.set PK_CELLX, PK_SCRATCH+89
.set PK_INDEX, PK_SCRATCH+92
; The background colour: the cell writer's clear colour (cells.S CS_CLEAR).
.set PK_BG, $ff
; The pixel band the quantizer reads (the mos-sim harness moves it).
.ifndef PIXELS
.set PIXELS, $8c00
.endif

.section .phase.pixels,"ax",@progbits

; void fast_pack_cell(uint8_t cell, uint8_t *bitmap_cell, uint8_t *screen_band,
;                     uint8_t *color_band)
.globl fast_pack_cell
.type fast_pack_cell,@function
fast_pack_cell:
    sta PK_INDEX
    ldx __rc2
    stx __rc8
    ldx __rc3
    stx __rc9
    clc
    adc __rc4
    sta __rc10
    sta __rc4
    lda __rc5
    adc #0
    sta __rc11
    sta __rc5
    lda PK_INDEX
    clc
    adc __rc6
    sta __rc12
    sta __rc6
    lda __rc7
    adc #0
    sta __rc13
    sta __rc7
    lda PK_INDEX
    asl
    asl
    sta PK_CELLX
    clc
    adc #<PIXELS
    sta __rc2
    lda #>PIXELS
    adc #0
    sta __rc3
.Lpk_cell:
    ; every slot starts "unused": the background colour
    lda PK_BG
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

    ; A single-colour cell needs neither ranking nor nearest-colour lookup:
    ; the background is code 0, any other colour keeps its preceding slot or
    ; takes slot 1.
    ldx #0
    lda PK_UNIFORM
    cmp PK_BG
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

    lda #0
    sta PK_NONBLACK
    ldx #15
.Lpk_clear_flags:
    sta PK_FLAGS,x
    sta PK_MAP,x
    dex
    bpl .Lpk_clear_flags
    lda PK_BG
    sta PK_CHOSEN
    sta PK_CHOSEN+1
    sta PK_CHOSEN+2
    ; A stable top-three insertion pass ranks each present colour once; the
    ; background is never ranked (it is code 0). MAP[0..3] temporarily holds
    ; score lows, MAP[3..6] the HUD priorities. Ascending colour iteration and
    ; strict comparison retain Rust's tie order.
    ldx #0
.Lpk_rank_color:
    cpx PK_BG
    beq .Lpk_rank_skip
    lda PK_COUNT,x
    bne .Lpk_rank_present
.Lpk_rank_skip:
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
    ; flag the chosen colours (an unfilled choice is the background: skip)
    ldy #2
.Lpk_flag_chosen:
    ldx PK_CHOSEN,y
    cpx PK_BG
    beq .Lpk_flag_next
    lda #1
    sta PK_FLAGS,x
.Lpk_flag_next:
    dey
    bpl .Lpk_flag_chosen
    lda #0
    ldx #5
.Lpk_clear_scores:
    sta PK_MAP,x
    dex
    bpl .Lpk_clear_scores

    ; Keep retained colors in their preceding-frame hardware slots (a
    ; preceding slot showing the background is never a chosen colour).
    ldx #0
.Lpk_retain_slot:
    ldy PK_OLD,x
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
    cmp PK_BG
    beq .Lpk_assign_next
    sta PK_COLOR
    tay
    lda PK_FLAGS,y
    and #2
    bne .Lpk_assign_next
    ldx #1
.Lpk_find_empty:
    lda PK_PAL,x
    cmp PK_BG
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
    ; This also handles every mixed cell with <=3 colours exactly.
    ldx #1
.Lpk_exact_slot:
    lda PK_PAL,x
    cmp PK_BG
    beq .Lpk_exact_next
    tay
    txa
    sta PK_MAP,y
.Lpk_exact_next:
    inx
    cpx #4
    bne .Lpk_exact_slot

    ; With at most three colours besides the background, exact-slot
    ; assignment covered every present colour.
    lda PK_NONBLACK
    cmp #4
    bcs .Lpk_omitted_colors
    jmp .Lpk_map_done
.Lpk_omitted_colors:

    ; COLOR_DISTANCE is a 16x16 little-endian u16 table shared with Rust. An
    ; omitted colour starts from its distance to the background (code 0).
    ldx #0
.Lpk_map_color:
    cpx PK_BG
    beq .Lpk_map_next
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
    lda PK_BG
    asl
    tay
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
    rts
.size fast_pack_cell, .-fast_pack_cell
.Lpk_patterns:
    .byte $00,$55,$aa,$ff
