; Cell-direct band rasterization: the native body of havoc::cells::CellBand.
; Spans write two-bit colour codes straight into per-cell bitmap bytes while
; a per-cell palette hands out the three hardware slots on first use. Code 0
; is the VIC background ($D021), which shows the frame's clear colour, so the
; clear colour and unpainted pixels need no slot; a cell asked for a fourth
; other colour (after recycling slots whose colour no longer appears in any
; row) is expanded into the pixel band at $8E00 and finished by the classic
; per-pixel quantizer (fast_pack_cell in pack.S).
;
; Slots store keys: the colour XOR the clear colour. The clear colour is key
; 0 like a free slot, so every zero test below stands for "the clear colour"
; or "free" without comparing against a variable; the finish XORs the keys
; back into colour nibbles (a free slot then shows the clear colour, which no
; code selects).
;
; State, PIXELS phase only:
;   CODES  $9400  8 rows x 40 cells, bits 7..6 = leftmost pixel, slot k = k*$55
;   COVER  $9540  8 rows x 40 masks: low nibble terrain, high nibble HUD;
;                bits 3 and 7 = leftmost pixel
;   PAL0/1/2 $9680/$96A8/$96D0  slot keys per cell, 0 = free ($FF x 3 once
;                                the cell lives in the pixel band)
;   HUDS   $7700  screen-byte staging only
;   OVER   $7728  per cell, nonzero once the cell lives in the pixel band
;   COVER_CLEAN $7750 holds $5A while COVER is known to be all zero
; After staging, COVER holds 320 cell-major bitmap bytes, HUDS holds 40 screen
; bytes and OVER holds 40 colour bytes. Staging consumes the band state; clear
; must run before any further painting or staging.
;   zero page $F0-$FF: the span's inputs and the hot per-cell temporaries;
;   $FE holds the span colour's key and $FF the band's clear colour (pack.S
;   reads it as the quantizer's background)
;   scratch $9300-$932F (GEOMETRY's kernel scratch, idle during PIXELS) for
;   the rare paths (spill, pixel form, glyphs, finish)
; Zero page: rc8/9 and rc10/11 hold the current CODES and COVER row (the
; raster steps them with its rows; the HUD entries set them per call); the
; raster's live state (rc2..7, rc12..19) is untouched except that the rare
; pixel paths save and restore rc2..5 around pointers of their own.
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
; The mos-sim harnesses relocate the band state and the two small tables (their
; own tables and images occupy the production addresses).
.ifndef CODES
.set CODES, $9400
.endif
.ifndef COVER
.set COVER, $9540
.endif
.ifndef PAL0
.set PAL0, $9680
.endif
.set PAL1, PAL0+40
.set PAL2, PAL0+80
.ifndef HUDS
.set HUDS, $7700
.endif
.ifndef OVER
.set OVER, $7728
.endif
.ifndef COVER_CLEAN
.set COVER_CLEAN, $7750
.endif
.ifndef PIXELS
.set PIXELS, $8e00
.endif
.ifndef CS_SCRATCH
.set CS_SCRATCH, $9300
.endif
.set CS_ROW, $f0
.set CS_FIRST, $f1
.set CS_END, $f2
.set CS_COLOR, $f3
.set CS_HUD, $f4
.set CS_MODE, $f5
.set CS_CELL, $f6
.set CS_LASTCELL, $f7
.set CS_MASK, $f8
.set CS_RIGHTMASK, $f9
.set CS_PAT, $fa
.set CS_BITS, $fb
.set CS_LIVE, $fc
.set CS_T, $fd
.set CS_KEY, $fe
.set CS_CLEAR, $ff
.set CS_CODES, CS_SCRATCH+$0e
.set CS_COVERN, CS_SCRATCH+$0f
.set CS_PIXEL, CS_SCRATCH+$00
.set CS_PALC, CS_SCRATCH+$11
.set CS_HUDC, CS_SCRATCH+$15
.set CS_SAVE, CS_SCRATCH+$16
.set CS_X, CS_SCRATCH+$1a
.set CS_GROW, CS_SCRATCH+$1b
.set CS_GBITS, CS_SCRATCH+$1c
.set CS_PTR, CS_SCRATCH+$1d
.set CS_EROW, CS_SCRATCH+$1f
.set CF_SCREEN, CS_SCRATCH+$22
.set CF_COLOR, CS_SCRATCH+$24
.set CF_CELL, CS_SCRATCH+$26
.set CF_CLEAR2, CS_SCRATCH+$27
.set CS_HUD_SEEN, CS_SCRATCH+$28

.section .phase.pixels.cells,"ax",@progbits

; Row pointers for CS_ROW into rc8/9 (CODES) and rc10/11 (COVER). The raster
; calls this once per triangle band and then steps the pointers by 40 per row.
.globl cell_row_pointers
cell_row_pointers:
.Lcs_pointers:
    ldx CS_ROW
    lda .Lcs_codes_lo,x
    sta __rc8
    lda .Lcs_codes_hi,x
    sta __rc9
    lda .Lcs_cover_lo,x
    sta __rc10
    lda .Lcs_cover_hi,x
    sta __rc11
    rts

; cell_rows: paint [CS_FIRST, CS_END) of the row selected by rc8..11 with
; CS_COLOR (CS_KEY = its key) / CS_HUD in CS_MODE (0 plain, 1 terrain-clear,
; 2 exterior). CS_END > CS_FIRST. The first and last cells take the general
; cell_row with their partial masks; the cells between them run a
; mode-specific loop that handles the common cases inline (cell not in pixel
; form, colour already in a slot, the clear colour, or a free slot to take)
; and falls back to cell_row for everything else.
.globl cell_rows
cell_rows:
    ; Once HUD ink appears, later unmarked writes must clear its pixel bits.
    lda CS_HUD
    beq .Lcs_hud_ready
    sta CS_HUD_SEEN
    lda #0
    sta COVER_CLEAN
.Lcs_hud_ready:
    ldx CS_END
    dex
    txa
    lsr
    lsr
    sta CS_LASTCELL
    txa
    and #3
    tax
    lda .Lcs_right,x
    sta CS_RIGHTMASK
    lda CS_FIRST
    and #3
    tax
    lda .Lcs_left,x
    sta CS_MASK
    lda CS_FIRST
    lsr
    lsr
    sta CS_CELL
    cmp CS_LASTCELL
    bne .Lcs_first
    lda CS_MASK
    and CS_RIGHTMASK
    sta CS_MASK
    ldy CS_CELL
    jmp cell_row
.Lcs_first:
    ldy CS_CELL
    lda CS_HUD_SEEN
    ora CS_MODE
    bne .Lcs_general_first
    ; plain ink without HUD marking: the edge cells merge through the mask
    ; tables unless they need the general path
    jsr .Lcs_edge
    iny
    jmp .Lcs_plain_loop
.Lcs_general_first:
    jsr cell_row
    ldy CS_CELL
    iny
    lda CS_HUD_SEEN
    bne .Lcs_general_interior
    lda CS_MODE
    cmp #1
    bne .Lcs_exterior_far
    jmp .Lcs_clear_interior
.Lcs_exterior_far:
    jmp .Lcs_exterior_loop
; After HUD ink appears, every cell takes the metadata-aware general path.
.Lcs_general_interior:
    cpy CS_LASTCELL
    beq .Lcs_last_cell
    sty CS_CELL
    lda #$0f
    sta CS_MASK
    jsr cell_row
    ldy CS_CELL
    iny
    bne .Lcs_general_interior
.Lcs_last_cell:
    sty CS_CELL
    lda CS_RIGHTMASK
    sta CS_MASK
    jmp cell_row

; An edge cell of a plain span: Y = CS_CELL, CS_MASK its mask. Preserves Y.
; A cell in the pixel form has its palette poisoned ($FF), so a key never
; matches and no free slot exists there: only the clear colour (key 0) must
; test OVER itself.
.Lcs_edge:
    ldx CS_MASK
    lda CS_KEY
    beq .Lcs_edge_clear
    cmp PAL0,y
    beq .Lcs_edge_1
    cmp PAL1,y
    beq .Lcs_edge_2
    cmp PAL2,y
    beq .Lcs_edge_3
    ; a colour the cell does not hold yet takes a free slot here; a full
    ; palette (recycling or the pixel form) goes the general way
    jsr .Lcs_take_slot
    bcc .Lcs_edge_general
    ldx CS_MASK
    cmp #$aa
    beq .Lcs_edge_2
    bcs .Lcs_edge_3
    bcc .Lcs_edge_1
.Lcs_edge_3:
    lda (__rc8),y
    ora .Lcs_expand,x
    sta (__rc8),y
    rts
.Lcs_edge_2:
    lda (__rc8),y
    and .Lcs_nbits,x
    ora .Lcs_pat2,x
    sta (__rc8),y
    rts
.Lcs_edge_1:
    lda (__rc8),y
    and .Lcs_nbits,x
    ora .Lcs_pat1,x
    sta (__rc8),y
    rts
.Lcs_edge_clear:
    lda OVER,y
    bne .Lcs_edge_general
    lda (__rc8),y
    and .Lcs_nbits,x
    sta (__rc8),y
    rts
.Lcs_edge_general:
    jsr cell_row
    ldy CS_CELL
    rts

; First use of a colour in cell Y (A = its key, nonzero): the first free
; slot takes it and the cell-wide pattern of that slot returns in A with the
; carry set; a full palette returns the carry clear and changes nothing, so
; the caller's general path can recycle a dead slot or spill the cell. The
; slot order matches cell_row's general assignment. Y is preserved.
.Lcs_take_slot:
    ldx PAL0,y
    beq .Lcs_take_0
    ldx PAL1,y
    beq .Lcs_take_1
    ldx PAL2,y
    beq .Lcs_take_2
    clc
    rts
.Lcs_take_0:
    sta PAL0,y
    lda #$55
    sec
    rts
.Lcs_take_1:
    sta PAL1,y
    lda #$aa
    sec
    rts
.Lcs_take_2:
    sta PAL2,y
    lda #$ff
    sec
    rts

; Plain interior cells.
.Lcs_plain_loop:
    cpy CS_LASTCELL
    beq .Lcs_plain_last
    lda CS_KEY
    beq .Lcs_plain_clear
    cmp PAL0,y
    beq .Lcs_plain_1
    cmp PAL1,y
    beq .Lcs_plain_2
    cmp PAL2,y
    beq .Lcs_plain_3
    jsr .Lcs_take_slot
    bcs .Lcs_plain_store
    bcc .Lcs_plain_general
.Lcs_plain_clear:
    lda OVER,y
    bne .Lcs_plain_general
    lda #0
    beq .Lcs_plain_store
.Lcs_plain_3:
    lda #$ff
    bne .Lcs_plain_store
.Lcs_plain_2:
    lda #$aa
    bne .Lcs_plain_store
.Lcs_plain_1:
    lda #$55
.Lcs_plain_store:
    sta (__rc8),y
    iny
    bne .Lcs_plain_loop
.Lcs_plain_general:
    sty CS_CELL
    lda #$0f
    sta CS_MASK
    jsr cell_row
    ldy CS_CELL
    iny
    bne .Lcs_plain_loop
.Lcs_plain_last:
    sty CS_CELL
    lda CS_RIGHTMASK
    sta CS_MASK
    jmp .Lcs_edge

; Terrain-clear interior cells: the whole cell row becomes covered.
.Lcs_clear_interior:
    lda #0
    sta COVER_CLEAN
.Lcs_clear_loop:
    cpy CS_LASTCELL
    bne .Lcs_clear_body
    jmp .Lcs_last_cell
.Lcs_clear_body:
    lda CS_KEY
    beq .Lcs_clear_clear
    cmp PAL0,y
    beq .Lcs_clear_1
    cmp PAL1,y
    beq .Lcs_clear_2
    cmp PAL2,y
    beq .Lcs_clear_3
    jsr .Lcs_take_slot
    bcs .Lcs_clear_store
    bcc .Lcs_clear_general
.Lcs_clear_clear:
    lda OVER,y
    bne .Lcs_clear_general
    lda #0
    beq .Lcs_clear_store
.Lcs_clear_3:
    lda #$ff
    bne .Lcs_clear_store
.Lcs_clear_2:
    lda #$aa
    bne .Lcs_clear_store
.Lcs_clear_1:
    lda #$55
.Lcs_clear_store:
    sta (__rc8),y
    lda #$0f
    sta (__rc10),y
    iny
    bne .Lcs_clear_loop
.Lcs_clear_general:
    sty CS_CELL
    lda #$0f
    sta CS_MASK
    jsr cell_row
    ldy CS_CELL
    iny
    bne .Lcs_clear_loop

; Exterior terrain interior cells: an uncovered cell row is painted whole,
; a partly covered one through the general mask path.
.Lcs_exterior_loop:
    cpy CS_LASTCELL
    bne .Lcs_exterior_body
    jmp .Lcs_last_cell
.Lcs_exterior_body:
    lda (__rc10),y
    bne .Lcs_exterior_general
    lda CS_KEY
    beq .Lcs_exterior_clear
    cmp PAL0,y
    beq .Lcs_exterior_1
    cmp PAL1,y
    beq .Lcs_exterior_2
    cmp PAL2,y
    beq .Lcs_exterior_3
    jsr .Lcs_take_slot
    bcs .Lcs_exterior_store
    bcc .Lcs_exterior_general
.Lcs_exterior_clear:
    lda OVER,y
    bne .Lcs_exterior_general
    lda #0
    beq .Lcs_exterior_store
.Lcs_exterior_3:
    lda #$ff
    bne .Lcs_exterior_store
.Lcs_exterior_2:
    lda #$aa
    bne .Lcs_exterior_store
.Lcs_exterior_1:
    lda #$55
.Lcs_exterior_store:
    sta (__rc8),y
    iny
    bne .Lcs_exterior_loop
.Lcs_exterior_general:
    sty CS_CELL
    lda #$0f
    sta CS_MASK
    jsr cell_row
    ldy CS_CELL
    iny
    bne .Lcs_exterior_loop
; cell_row: paint the CS_MASK pixels of cell Y on the row selected by rc8..11.
; A, X and Y are clobbered. Plain ink whose colour the cell already holds
; (or the clear colour) merges through the mask tables directly; everything
; else — coverage modes, HUD marking, slot assignment, recycling, the pixel
; form — takes the general path below.
cell_row:
    lda OVER,y
    bne .Lcr_pixels_far
    lda CS_MODE
    ora CS_HUD_SEEN
    bne .Lcr_cell
    ldx CS_MASK
    lda CS_KEY
    beq .Lcr_fast_0
    cmp PAL0,y
    beq .Lcr_fast_1
    cmp PAL1,y
    beq .Lcr_fast_2
    cmp PAL2,y
    beq .Lcr_fast_3
    ; a colour the cell does not hold yet takes a free slot; a full palette
    ; (recycling or the pixel form) is the general path's business
    jsr .Lcs_take_slot
    bcc .Lcr_cell
    ldx CS_MASK
    cmp #$aa
    beq .Lcr_fast_2
    bcs .Lcr_fast_3
    bcc .Lcr_fast_1
.Lcr_fast_3:
    lda (__rc8),y
    ora .Lcs_expand,x
    sta (__rc8),y
    rts
.Lcr_fast_2:
    lda (__rc8),y
    and .Lcs_nbits,x
    ora .Lcs_pat2,x
    sta (__rc8),y
    rts
.Lcr_fast_1:
    lda (__rc8),y
    and .Lcs_nbits,x
    ora .Lcs_pat1,x
    sta (__rc8),y
    rts
.Lcr_fast_0:
    lda (__rc8),y
    and .Lcs_nbits,x
    sta (__rc8),y
    rts
.Lcr_pixels_far:
    jmp .Lcr_pixels
.Lcr_cell:
    lda CS_MODE
    cmp #2
    bne .Lcr_colour
    ; exterior terrain paints only uncovered pixels
    lda (__rc10),y
    eor #$ff
    and CS_MASK
    sta CS_MASK
    bne .Lcr_colour
    rts
.Lcr_colour:
    lda CS_KEY
    bne .Lcr_find
    ldx #0
    jmp .Lcr_write
.Lcr_find:
    cmp PAL0,y
    bne .Lcr_not1
    ldx #1
    jmp .Lcr_slot
.Lcr_not1:
    cmp PAL1,y
    bne .Lcr_not2
    ldx #2
    jmp .Lcr_slot
.Lcr_not2:
    cmp PAL2,y
    bne .Lcr_assign
    ldx #3
    jmp .Lcr_slot
.Lcr_assign:
    ldx PAL0,y
    bne .Lcr_full1
    sta PAL0,y
    ldx #1
    jmp .Lcr_slot
.Lcr_full1:
    ldx PAL1,y
    bne .Lcr_full2
    sta PAL1,y
    ldx #2
    jmp .Lcr_slot
.Lcr_full2:
    ldx PAL2,y
    bne .Lcr_recycle
    sta PAL2,y
    ldx #3
    jmp .Lcr_slot
.Lcr_recycle:
    ; A slot whose colour appears in no row any more can be reused.
    ldx CODES,y
    lda .Lcs_present,x
    sta CS_LIVE
    ldx CODES+40,y
    lda .Lcs_present,x
    ora CS_LIVE
    ldx CODES+80,y
    ora .Lcs_present,x
    ldx CODES+120,y
    ora .Lcs_present,x
    ldx CODES+160,y
    ora .Lcs_present,x
    ldx CODES+200,y
    ora .Lcs_present,x
    ldx CODES+240,y
    ora .Lcs_present,x
    ldx CODES+280,y
    ora .Lcs_present,x
    sta CS_LIVE
    and #2
    bne .Lcr_live1
    lda CS_KEY
    sta PAL0,y
    ldx #1
    jmp .Lcr_slot
.Lcr_live1:
    lda CS_LIVE
    and #4
    bne .Lcr_live2
    lda CS_KEY
    sta PAL1,y
    ldx #2
    jmp .Lcr_slot
.Lcr_live2:
    lda CS_LIVE
    and #8
    bne .Lcr_spill
    lda CS_KEY
    sta PAL2,y
    ldx #3
    jmp .Lcr_slot
.Lcr_spill:
    jsr .Lcr_expand
    jmp .Lcr_pixels
.Lcr_slot:
.Lcr_write:
    lda .Lcs_pattern,x
    sta CS_PAT
    ldx CS_MASK
    lda .Lcs_expand,x
    sta CS_BITS
    ; replace the masked code bits: ((codes ^ pattern) & bits) ^ codes
    lda (__rc8),y
    sta CS_T
    eor CS_PAT
    and CS_BITS
    eor CS_T
    sta (__rc8),y
    lda CS_HUD_SEEN
    beq .Lcr_cover_mode
    ; Replace only the written pixels' HUD bits, including background ink.
    lda CS_MASK
    asl
    asl
    asl
    asl
    sta CS_T
    eor #$ff
    and (__rc10),y
    ldx CS_HUD
    beq .Lcr_hud_store
    ora CS_T
.Lcr_hud_store:
    sta (__rc10),y
.Lcr_cover_mode:
    lda CS_MODE
    cmp #1
    beq .Lcr_cover_set
    cmp #0
    bne .Lcr_done
    lda CS_HUD
    beq .Lcr_done
    ; Match pixel form: plain HUD ink replaces terrain coverage too.
    lda CS_MASK
    eor #$ff
    and (__rc10),y
    sta (__rc10),y
    rts
.Lcr_cover_set:
    lda (__rc10),y
    ora CS_MASK
    sta (__rc10),y
    lda #0
    sta COVER_CLEAN
.Lcr_done:
    rts

; The classic per-pixel rules for a cell living in the pixel band. A whole
; cell row of plain or terrain ink (the interior cells of spans) takes the
; unrolled paths; partial masks and HUD ink go pixel by pixel.
.Lcr_pixels:
    lda __rc2
    sta CS_SAVE
    lda __rc3
    sta CS_SAVE+1
    ldx CS_ROW
    lda .Lcs_pixel_lo,x
    sta __rc2
    lda .Lcs_pixel_hi,x
    sta __rc3
    tya
    asl
    asl
    tay
    lda CS_MASK
    cmp #$0f
    bne .Lcr_pixel_masked
    lda CS_HUD
    bne .Lcr_pixel_masked
    lda CS_MODE
    beq .Lcr_pixel_plain_row
    cmp #1
    beq .Lcr_pixel_clear_row
    ; exterior terrain: only uncovered pixels take the colour
    lda (__rc2),y
    and #$40
    bne .Lcr_pixel_exterior_1
    lda CS_COLOR
    sta (__rc2),y
.Lcr_pixel_exterior_1:
    iny
    lda (__rc2),y
    and #$40
    bne .Lcr_pixel_exterior_2
    lda CS_COLOR
    sta (__rc2),y
.Lcr_pixel_exterior_2:
    iny
    lda (__rc2),y
    and #$40
    bne .Lcr_pixel_exterior_3
    lda CS_COLOR
    sta (__rc2),y
.Lcr_pixel_exterior_3:
    iny
    lda (__rc2),y
    and #$40
    bne .Lcr_pixels_exit
    lda CS_COLOR
    sta (__rc2),y
.Lcr_pixels_exit:
    jmp .Lcr_pixels_done
.Lcr_pixel_clear_row:
    lda CS_COLOR
    ora #$40
    sta (__rc2),y
    iny
    sta (__rc2),y
    iny
    sta (__rc2),y
    iny
    sta (__rc2),y
    jmp .Lcr_pixels_done
.Lcr_pixel_plain_row:
    ; keep each pixel's coverage bit under the new colour
    lda (__rc2),y
    and #$40
    ora CS_COLOR
    sta (__rc2),y
    iny
    lda (__rc2),y
    and #$40
    ora CS_COLOR
    sta (__rc2),y
    iny
    lda (__rc2),y
    and #$40
    ora CS_COLOR
    sta (__rc2),y
    iny
    lda (__rc2),y
    and #$40
    ora CS_COLOR
    sta (__rc2),y
    jmp .Lcr_pixels_done
.Lcr_pixel_masked:
    ; the mask nibble shifts out of bit 7, leftmost pixel first
    lda CS_MASK
    asl
    asl
    asl
    asl
    sta CS_T
    ldx #4
.Lcr_pixel:
    asl CS_T
    bcc .Lcr_pixel_next
    lda CS_MODE
    beq .Lcr_pixel_plain
    cmp #1
    beq .Lcr_pixel_clear
    lda (__rc2),y
    and #$40
    bne .Lcr_pixel_next
    lda CS_COLOR
    ora CS_HUD
    sta (__rc2),y
    jmp .Lcr_pixel_next
.Lcr_pixel_clear:
    lda CS_COLOR
    ora CS_HUD
    ora #$40
    sta (__rc2),y
    jmp .Lcr_pixel_next
.Lcr_pixel_plain:
    lda CS_HUD
    bne .Lcr_pixel_hud
    lda (__rc2),y
    and #$40
    ora CS_COLOR
    sta (__rc2),y
    jmp .Lcr_pixel_next
.Lcr_pixel_hud:
    lda CS_COLOR
    ora #$80
    sta (__rc2),y
.Lcr_pixel_next:
    iny
    dex
    bne .Lcr_pixel
.Lcr_pixels_done:
    lda CS_SAVE
    sta __rc2
    lda CS_SAVE+1
    sta __rc3
    rts

; Expand cell Y into its pixel form (all eight rows) and mark it overflowed.
; Y is preserved. The four slot colours (slot 0 is the clear colour;
; the keys XOR back into colours) are built
; once; every row then decodes its code byte through them and adds the
; terrain and HUD bits of its COVER nibbles.
.Lcr_expand:
    sty CS_CELL
    lda #1
    sta OVER,y
    lda CS_CLEAR
    sta CS_PALC
    lda PAL0,y
    eor CS_CLEAR
    sta CS_PALC+1
    lda PAL1,y
    eor CS_CLEAR
    sta CS_PALC+2
    lda PAL2,y
    eor CS_CLEAR
    sta CS_PALC+3
    ; poison the palette: the span fast paths then never match or take a
    ; slot here and route every nonblack write to the pixel form
    lda #$ff
    sta PAL0,y
    sta PAL1,y
    sta PAL2,y
    lda __rc2
    sta CS_SAVE
    lda __rc3
    sta CS_SAVE+1
    lda __rc4
    sta CS_SAVE+2
    lda __rc5
    sta CS_SAVE+3
    tya
    asl
    asl
    sta CS_PTR              ; 4 * cell: the pixel column of the row
    lda #0
    sta CS_EROW
.Lcr_expand_row:
    ldx CS_EROW
    lda .Lcs_codes_lo,x
    sta __rc4
    lda .Lcs_codes_hi,x
    sta __rc5
    lda .Lcs_cover_lo,x
    sta __rc2
    lda .Lcs_cover_hi,x
    sta __rc3
    ldy CS_CELL
    lda (__rc2),y
    sta CS_HUDC            ; HUD nibble already in bits 7..4
    asl
    asl
    asl
    asl
    sta CS_COVERN           ; coverage nibble in bits 7..4, leftmost first
    lda (__rc4),y
    sta CS_CODES
    ; pixel 0: bits 7..6
    rol
    rol
    rol
    and #3
    tax
    lda CS_PALC,x
    asl CS_COVERN
    bcc .Lcr_expand_p0
    ora #$40
.Lcr_expand_p0:
    asl CS_HUDC
    bcc .Lcr_expand_h0
    ora #$80
.Lcr_expand_h0:
    sta CS_PIXEL
    ; pixel 1: bits 5..4
    lda CS_CODES
    lsr
    lsr
    lsr
    lsr
    and #3
    tax
    lda CS_PALC,x
    asl CS_COVERN
    bcc .Lcr_expand_p1
    ora #$40
.Lcr_expand_p1:
    asl CS_HUDC
    bcc .Lcr_expand_h1
    ora #$80
.Lcr_expand_h1:
    sta CS_PIXEL+1
    ; pixel 2: bits 3..2
    lda CS_CODES
    lsr
    lsr
    and #3
    tax
    lda CS_PALC,x
    asl CS_COVERN
    bcc .Lcr_expand_p2
    ora #$40
.Lcr_expand_p2:
    asl CS_HUDC
    bcc .Lcr_expand_h2
    ora #$80
.Lcr_expand_h2:
    sta CS_PIXEL+2
    ; pixel 3: bits 1..0
    lda CS_CODES
    and #3
    tax
    lda CS_PALC,x
    asl CS_COVERN
    bcc .Lcr_expand_p3
    ora #$40
.Lcr_expand_p3:
    asl CS_HUDC
    bcc .Lcr_expand_h3
    ora #$80
.Lcr_expand_h3:
    sta CS_PIXEL+3
    ; store the row
    ldx CS_EROW
    lda .Lcs_pixel_lo,x
    sta __rc2
    lda .Lcs_pixel_hi,x
    sta __rc3
    ldy CS_PTR
    lda CS_PIXEL
    sta (__rc2),y
    iny
    lda CS_PIXEL+1
    sta (__rc2),y
    iny
    lda CS_PIXEL+2
    sta (__rc2),y
    iny
    lda CS_PIXEL+3
    sta (__rc2),y
    inc CS_EROW
    lda CS_EROW
    cmp #8
    beq .Lcr_expand_done
    jmp .Lcr_expand_row
.Lcr_expand_done:
    lda CS_SAVE
    sta __rc2
    lda CS_SAVE+1
    sta __rc3
    lda CS_SAVE+2
    sta __rc4
    lda CS_SAVE+3
    sta __rc5
    ldy CS_CELL
    rts

; void fast_cell_clear(uint8_t color): start a band filled with `color`.
; C ABI: color in A. Remembers the clear colour (code 0, the VIC background)
; and clears CODES, the palettes (every slot free), HUD-seen and overflow
; flags; COVER only when terrain or HUD metadata dirtied it since the last clear.
.globl fast_cell_clear
.type fast_cell_clear,@function
fast_cell_clear:
    and #15
    sta CS_CLEAR
    lda #0
    sta CS_HUD_SEEN
    ; eight rows per step: 320 code bytes in forty steps
    ldx #39
.Lcc_codes:
    sta CODES,x
    sta CODES+40,x
    sta CODES+80,x
    sta CODES+120,x
    sta CODES+160,x
    sta CODES+200,x
    sta CODES+240,x
    sta CODES+280,x
    dex
    bpl .Lcc_codes
    ; the palettes and the overflow flags start clear (five
    ; eight-cell steps)
    ldx #7
.Lcc_zero:
    sta PAL0,x
    sta PAL0+8,x
    sta PAL0+16,x
    sta PAL0+24,x
    sta PAL0+32,x
    sta PAL1,x
    sta PAL1+8,x
    sta PAL1+16,x
    sta PAL1+24,x
    sta PAL1+32,x
    sta PAL2,x
    sta PAL2+8,x
    sta PAL2+16,x
    sta PAL2+24,x
    sta PAL2+32,x
    sta OVER,x
    sta OVER+8,x
    sta OVER+16,x
    sta OVER+24,x
    sta OVER+32,x
    dex
    bpl .Lcc_zero
    lda COVER_CLEAN
    cmp #$5a
    beq .Lcc_done
    lda #0
    ldx #39
.Lcc_cover:
    sta COVER,x
    sta COVER+40,x
    sta COVER+80,x
    sta COVER+120,x
    sta COVER+160,x
    sta COVER+200,x
    sta COVER+240,x
    sta COVER+280,x
    dex
    bpl .Lcc_cover
    lda #$5a
    sta COVER_CLEAN
.Lcc_done:
    rts
.size fast_cell_clear, .-fast_cell_clear

; void fast_cell_span(uint8_t row, uint8_t first, uint8_t end, uint8_t color)
; C ABI: row A, first X, end rc2, color rc3 (bit 7 = HUD ink). Plain paint.
.globl fast_cell_span
.type fast_cell_span,@function
fast_cell_span:
    sta CS_ROW
    stx CS_FIRST
    cpx __rc2
    bcs .Lsp_empty
    lda __rc2
    sta CS_END
    lda __rc3
    and #15
    sta CS_COLOR
    eor CS_CLEAR
    sta CS_KEY
    lda __rc3
    and #$80
    sta CS_HUD
    lda #0
    sta CS_MODE
    jsr .Lcs_pointers
    jmp cell_rows
.Lsp_empty:
    rts
.size fast_cell_span, .-fast_cell_span

; void fast_cell_plot(uint8_t row, uint8_t x, uint8_t color)
; C ABI: row A, x X, color rc2 (bit 7 = HUD ink).
.globl fast_cell_plot
.type fast_cell_plot,@function
fast_cell_plot:
    sta CS_ROW
    lda __rc2
    and #15
    sta CS_COLOR
    eor CS_CLEAR
    sta CS_KEY
    lda __rc2
    and #$80
    sta CS_HUD
    beq .Lpl_hud_ready
    sta CS_HUD_SEEN
    lda #0
    sta COVER_CLEAN
.Lpl_hud_ready:
    lda #0
    sta CS_MODE
    txa
    and #3
    tay
    lda .Lcs_single,y
    sta CS_MASK
    txa
    lsr
    lsr
    sta CS_CELL
    jsr .Lcs_pointers
    ldy CS_CELL
    jmp cell_row
.size fast_cell_plot, .-fast_cell_plot

; void fast_cell_glyph(uint8_t row, uint8_t x, const uint8_t rows[5], uint8_t color)
; C ABI: row A, x X, rows rc2/3, color rc4. The caller proves that the five
; rows and three columns lie inside the band. Each glyph row becomes at most
; two cell masks: (bits << 5) >> (x & 3) holds the first cell's mask in its
; high nibble and the spill into the next cell in its low nibble.
.globl fast_cell_glyph
.type fast_cell_glyph,@function
fast_cell_glyph:
    sta CS_ROW
    stx CS_X
    lda __rc4
    and #15
    sta CS_COLOR
    eor CS_CLEAR
    sta CS_KEY
    lda __rc4
    and #$80
    sta CS_HUD
    beq .Lgl_hud_ready
    sta CS_HUD_SEEN
    lda #0
    sta COVER_CLEAN
.Lgl_hud_ready:
    lda #0
    sta CS_MODE
    sta CS_GROW
.Lgl_row:
    ldy CS_GROW
    lda (__rc2),y
    beq .Lgl_next
    asl
    asl
    asl
    asl
    asl
    sta CS_GBITS
    lda CS_X
    and #3
    beq .Lgl_shifted
    tax
.Lgl_shift:
    lsr CS_GBITS
    dex
    bne .Lgl_shift
.Lgl_shifted:
    jsr .Lcs_pointers
    lda CS_X
    lsr
    lsr
    sta CS_CELL
    lda CS_GBITS
    lsr
    lsr
    lsr
    lsr
    beq .Lgl_second
    sta CS_MASK
    ldy CS_CELL
    jsr cell_row
.Lgl_second:
    lda CS_GBITS
    and #$0f
    beq .Lgl_next
    sta CS_MASK
    ldy CS_CELL
    iny
    jsr cell_row
.Lgl_next:
    inc CS_ROW
    inc CS_GROW
    lda CS_GROW
    cmp #5
    bne .Lgl_row
    rts
.size fast_cell_glyph, .-fast_cell_glyph

; void fast_cell_stage(const uint8_t *old_screen, const uint8_t *old_colors)
; C ABI: preceding screen rc2/3 and colours rc4/5 (40 bytes each, read only).
; Outputs reuse COVER (320 cell-major bitmap bytes), HUDS (40 screen bytes)
; and OVER (40 colour bytes). Each overflow flag is consumed before its byte
; is reused. For overflowed cells, seed only that cell's staged attributes
; with the old values so fast_pack_cell retains its in-place palette contract.
; Slot keys XOR back into colour nibbles; a free slot shows the clear colour.
; No live output is written here. The caller must publish before clearing.
.globl fast_cell_stage
.type fast_cell_stage,@function
fast_cell_stage:
    lda __rc2
    sta CF_SCREEN
    lda __rc3
    sta CF_SCREEN+1
    lda __rc4
    sta CF_COLOR
    sta __rc6
    lda __rc5
    sta CF_COLOR+1
    sta __rc7
    lda __rc2
    sta __rc4
    lda __rc3
    sta __rc5
    lda #<COVER
    sta __rc2
    lda #>COVER
    sta __rc3
    ; COVER no longer contains terrain masks, even if no terrain dirtied it.
    lda #0
    sta COVER_CLEAN
    ; the clear colour in both nibbles unkeys a screen byte in one eor
    lda CS_CLEAR
    asl
    asl
    asl
    asl
    ora CS_CLEAR
    sta CF_CLEAR2
    lda #0
    sta CF_CELL
.Lcf_cell:
    ldx CF_CELL
    lda OVER,x
    bne .Lcf_overflow
    lda CODES,x
    ldy #0
    sta (__rc2),y
    lda CODES+40,x
    iny
    sta (__rc2),y
    lda CODES+80,x
    iny
    sta (__rc2),y
    lda CODES+120,x
    iny
    sta (__rc2),y
    lda CODES+160,x
    iny
    sta (__rc2),y
    lda CODES+200,x
    iny
    sta (__rc2),y
    lda CODES+240,x
    iny
    sta (__rc2),y
    lda CODES+280,x
    iny
    sta (__rc2),y
    txa
    tay
    lda PAL0,x
    asl
    asl
    asl
    asl
    ora PAL1,x
    eor CF_CLEAR2
    sta HUDS,y
    lda PAL2,x
    eor CS_CLEAR
    sta OVER,y
    clc
    lda __rc2
    adc #8
    sta __rc2
    bcc .Lcf_next
    inc __rc3
.Lcf_next:
    inc CF_CELL
.Lcf_test:
    lda CF_CELL
    cmp #40
    beq .Lcf_done
    jmp .Lcf_cell
.Lcf_done:
    rts
.Lcf_overflow:
    ; Preserve unvisited OVER flags: preload old attributes one cell at a time.
    txa
    tay
    lda (__rc4),y
    sta HUDS,y
    lda (__rc6),y
    sta OVER,y
    lda #<HUDS
    sta __rc4
    lda #>HUDS
    sta __rc5
    lda #<OVER
    sta __rc6
    lda #>OVER
    sta __rc7
    txa
    jsr fast_pack_cell
    ; the quantizer clobbers rc2..13: restore the old-attribute pointers and
    ; point the bitmap at the next cell
    lda CF_SCREEN
    sta __rc4
    lda CF_SCREEN+1
    sta __rc5
    lda CF_COLOR
    sta __rc6
    lda CF_COLOR+1
    sta __rc7
    inc CF_CELL
    lda #0
    sta CS_T
    lda CF_CELL
    asl
    rol CS_T
    asl
    rol CS_T
    asl
    rol CS_T
    clc
    adc #<COVER
    sta __rc2
    lda CS_T
    adc #>COVER
    sta __rc3
    jmp .Lcf_test
.size fast_cell_stage, .-fast_cell_stage

.Lcs_pattern:
    .byte $00,$55,$aa,$ff
.Lcs_left:
    .byte $0f,$07,$03,$01
.Lcs_right:
    .byte $08,$0c,$0e,$0f
.Lcs_single:
    .byte $08,$04,$02,$01
.Lcs_expand:
    .byte $00,$03,$0c,$0f,$30,$33,$3c,$3f,$c0,$c3,$cc,$cf,$f0,$f3,$fc,$ff
; The inverse mask and the slot 1 / slot 2 patterns under each mask.
.Lcs_nbits:
    .byte $ff,$fc,$f3,$f0,$cf,$cc,$c3,$c0,$3f,$3c,$33,$30,$0f,$0c,$03,$00
.Lcs_pat1:
    .byte $00,$01,$04,$05,$10,$11,$14,$15,$40,$41,$44,$45,$50,$51,$54,$55
.Lcs_pat2:
    .byte $00,$02,$08,$0a,$20,$22,$28,$2a,$80,$82,$88,$8a,$a0,$a2,$a8,$aa
.Lcs_codes_lo:
    .byte <(CODES),<(CODES+40),<(CODES+80),<(CODES+120),<(CODES+160),<(CODES+200),<(CODES+240),<(CODES+280)
.Lcs_codes_hi:
    .byte >(CODES),>(CODES+40),>(CODES+80),>(CODES+120),>(CODES+160),>(CODES+200),>(CODES+240),>(CODES+280)
.Lcs_cover_lo:
    .byte <(COVER),<(COVER+40),<(COVER+80),<(COVER+120),<(COVER+160),<(COVER+200),<(COVER+240),<(COVER+280)
.Lcs_cover_hi:
    .byte >(COVER),>(COVER+40),>(COVER+80),>(COVER+120),>(COVER+160),>(COVER+200),>(COVER+240),>(COVER+280)
.Lcs_pixel_lo:
    .byte <(PIXELS),<(PIXELS+160),<(PIXELS+320),<(PIXELS+480),<(PIXELS+640),<(PIXELS+800),<(PIXELS+960),<(PIXELS+1120)
.Lcs_pixel_hi:
    .byte >(PIXELS),>(PIXELS+160),>(PIXELS+320),>(PIXELS+480),>(PIXELS+640),>(PIXELS+800),>(PIXELS+960),>(PIXELS+1120)
; Bit k set when code k occurs in a code byte (index = the byte).
.Lcs_present:
    .irp hi,0,1,2,3
    .irp mid,0,1,2,3
    .irp lo2,0,1,2,3
    .irp lo,0,1,2,3
    .byte (1<<\hi)|(1<<\mid)|(1<<\lo2)|(1<<\lo)
    .endr
    .endr
    .endr
    .endr
