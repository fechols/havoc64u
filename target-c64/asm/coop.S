; Two-player glue for the one resident Game at __game_state_start ($C800).
; The first player's ship opens Game; the second's (`reserve`) follows it.
; Absolute addressing keeps these SIM helpers small. game.rs checks every
; offset below with const assertions on this target.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7
.zeropage __rc8

.set SHIP_SIZE, 49
.set RESERVE, 49
.set DEVICES, 98
.set SWAPPED, 99
.set PLAYER_X, 15
.set PLAYER_Y, 17
.set LIVES, 40
.set RESPAWN, 47
.set G, __game_state_start

; void coop_swap(Game *game): exchange the two ships; toggle `swapped`.
; Clobbers A, X, Y only. Resident: SIM and the PIXELS HUD both swap seats.
.section .text.coop_swap,"ax",@progbits
.globl coop_swap
.type coop_swap,@function
coop_swap:
    ldx #SHIP_SIZE - 1
.Lswap_byte:
    lda G,x
    ldy G + RESERVE,x
    sta G + RESERVE,x
    tya
    sta G,x
    dex
    bpl .Lswap_byte
    lda G + SWAPPED
    eor #1
    sta G + SWAPPED
    rts
.size coop_swap, .-coop_swap

.section .phase.sim_extra,"ax",@progbits

; void coop_each(Game *game, void (*action)(Game *), uint8_t ships)
; C ABI: game rc2/3, action rc4/5, ships in A. Calls action(game) for the
; first ship (bit 0), then with the second swapped in front (bit 1). The
; action and mask stay on the hardware stack, so actions may nest.
.globl coop_each
.type coop_each,@function
coop_each:
    pha
    lda __rc5
    pha
    lda __rc4
    pha
    ; Stack: $101,x action low, $102,x high, $103,x ships.
    tsx
    lda $103,x
    and #1
    beq .Leach_second
    jsr .Leach_call
.Leach_second:
    tsx
    lda $103,x
    and #2
    beq .Leach_done
    jsr coop_swap
    jsr .Leach_call
    jsr coop_swap
.Leach_done:
    pla
    pla
    pla
    rts
; Below our return address: $103,x action low, $104,x high.
.Leach_call:
    tsx
    lda $103,x
    sta __rc4
    lda $104,x
    sta __rc5
    lda #<G
    sta __rc2
    lda #>G
    sta __rc3
    jmp (__rc4)
.size coop_each, .-coop_each

; uint8_t coop_near(const Vec3 *pos, uint8_t radius)
; C ABI: pos rc2/3, radius in A. Bit 0: the first ship flies within radius
; of pos in both X and Y; bit 1: likewise the seated second ship. A ship
; flies when it has lives and no pending return delay. rc6..8 are scratch.
; Reads only; no Game pointer is passed because reloading it costs ~16 B a
; call site in the full SIM overlays (callers hold no unstored ship writes).
.globl coop_near
.type coop_near,@function
coop_near:
    sta __rc6
    ldx #0
    jsr .Lnear_ship
    lda #0
    rol
    sta __rc8
    lda G + DEVICES
    cmp #$10
    bcc .Lnear_done
    ldx #RESERVE
    jsr .Lnear_ship
    bcc .Lnear_done
    lda __rc8
    ora #2
    sta __rc8
.Lnear_done:
    lda __rc8
    rts
; Carry set when the ship at G + X is flying and close to pos.
.Lnear_ship:
    lda G + LIVES,x
    beq .Lnear_no
    lda G + RESPAWN,x
    bne .Lnear_no
    ldy #0
    jsr .Lnear_axis
    bcc .Lnear_no
    ldy #2
    inx
    inx
    jsr .Lnear_axis
    dex
    dex
    rts
.Lnear_no:
    clc
    rts
; Carry set when |pos[Y/2] - ship[X]| <= radius (16-bit difference).
.Lnear_axis:
    lda (__rc2),y
    sec
    sbc G + PLAYER_X,x
    sta __rc7
    iny
    lda (__rc2),y
    sbc G + PLAYER_X + 1,x
    beq .Lnear_positive
    cmp #$ff
    bne .Lnear_far
    ; -radius <= difference < 0: low byte + radius carries out.
    lda __rc7
    clc
    adc __rc6
    rts
.Lnear_positive:
    lda __rc6
    cmp __rc7
    rts
.Lnear_far:
    clc
    rts
.size coop_near, .-coop_near

; ---------------------------------------------------------------- ship state
; Field offsets inside a ship record (game.rs Ship, and Controls as laid out
; on this target; game.rs asserts each one).
.set C_NOW, 0
.set C_RELEASED, 2
.set C_FLAGS, 11
.set C_CHARGE, 12
.set C_ROLL_PENDING, 14
.set PLAYER_Z, 19
.set FLIGHT, 21
.set SCORE, 31
.set BOOST_CYCLE, 35
.set SHIELD, 37
.set BOMBS, 38
.set SHOT_COOLDOWN, 41
.set INVULN, 42
.set HIT_FLASH, 43
.set ROLL_TICKS, 44
.set ROLL_COOLDOWN, 45
.set VIEW, 100
.set HIT_AMOUNT, 104
.set SHIP_BANK, 112
.set AIM, 114
.set AIM_TURN, 118
.set START_LIVES, 119
.set STAGED, 120
.set MODE, 128
.set MODE_RESPAWNING, 2
.set FIRE, $10
.set BOOST_BIT, $20
.set DEVICE_KEYS, 3
.set HIT_AT, 106
; Mode::Playing and Mode::Transition; BOOST_TICKS + BOOST_COOLDOWN_TICKS;
; Effect::Roll.
.set MODE_PLAYING, 1
.set MODE_TRANSITION, 6
.set BOOST_FULL, 300
.set BOOST_COOLDOWN, 200
.set EFFECT_ROLL, 5
; Controls::flags bits kept by clear_pending (the three tap arms and
; BOOST_ACTIVE), and those kept by a reset (all but BOOST_ACTIVE and the arms).
.set KEEP_PENDING, $87
.set KEEP_RESET, $78
.set SECOND_SHIP_X, 48
.set PLAYER_Z_VALUE, 180

; Carry set when the reserve ship holds a seated player: the second player,
; or the first while swapped. Clobbers A.
.Lreserve_present:
    lda G + SWAPPED
    bne .Lpresent_yes
    lda G + DEVICES
    cmp #$10
    rts
.Lpresent_yes:
    sec
    rts
; Carry set when the reserve ship is seated, has lives and is not waiting.
.Lreserve_alive:
    jsr .Lreserve_present
    bcc .Lalive_done
    lda G + RESERVE + LIVES
    beq .Lalive_no
    lda G + RESERVE + RESPAWN
    bne .Lalive_no
    sec
.Lalive_done:
    rts
.Lalive_no:
    clc
    rts

; void coop_reset_ship(Game *): Game::reset_ship for the ship in front.
.globl coop_reset_ship
.type coop_reset_ship,@function
coop_reset_ship:
    lda #0
    ldx #9
.Lreset_flight:
    sta G + FLIGHT,x
    dex
    bpl .Lreset_flight
    sta G + PLAYER_X + 1
    sta G + PLAYER_Y
    sta G + PLAYER_Y + 1
    sta G + PLAYER_Z + 1
    sta G + BOOST_CYCLE
    sta G + BOOST_CYCLE + 1
    sta G + HIT_FLASH
    sta G + ROLL_TICKS
    sta G + ROLL_COOLDOWN
    sta G + SHOT_COOLDOWN
    sta G + RESPAWN
    sta G + C_CHARGE
    ldx G + SWAPPED
    beq .Lreset_lane
    lda #SECOND_SHIP_X
.Lreset_lane:
    sta G + PLAYER_X
    lda #PLAYER_Z_VALUE
    sta G + PLAYER_Z
    lda #100
    sta G + SHIELD
    sta G + INVULN
    lda G + BOMBS
    bne .Lreset_bombs
    inc G + BOMBS
.Lreset_bombs:
    lda G + C_FLAGS
    and #KEEP_RESET
    sta G + C_FLAGS
    lda G + C_NOW
    sta G + C_RELEASED
    lda G + C_NOW + 1
    sta G + C_RELEASED + 1
    rts
.size coop_reset_ship, .-coop_reset_ship

; void coop_settle_ship(Game *): Controls::clear_pending for the front ship.
.globl coop_settle_ship
.type coop_settle_ship,@function
coop_settle_ship:
.Lsettle:
    lda G + C_FLAGS
    and #KEEP_PENDING
    sta G + C_FLAGS
    lda #0
    sta G + C_ROLL_PENDING
    rts
.size coop_settle_ship, .-coop_settle_ship

; void coop_fly_ship(Game *): one tick of the front ship. A downed co-op ship
; counts down its return; a flying one takes its controls; gestures retire.
.globl coop_fly_ship
.type coop_fly_ship,@function
coop_fly_ship:
    lda G + RESPAWN
    beq .Lfly_live
    dec G + RESPAWN
    bne .Lsettle
    jsr coop_reset_ship
    jmp coop_settle_ship
.Lfly_live:
    lda G + LIVES
    beq .Lsettle
    lda G + SHIP_BANK
    sta __rc4
    lda G + SHIP_BANK + 1
    sta __rc5
    lda #<G
    sta __rc2
    lda #>G
    sta __rc3
    jsr game_update_player_controls
    jmp coop_settle_ship
.size coop_fly_ship, .-coop_fly_ship

; void coop_update_ships(Game *, const AssetBank *bank, bool fly)
; C ABI: game rc2/3, bank rc4/5, fly in A. Game::update_ships: every seated
; ship flies (or only settles), then the co-op camera target is cached.
.globl coop_update_ships
.type coop_update_ships,@function
coop_update_ships:
    tax
    lda __rc4
    sta G + SHIP_BANK
    lda __rc5
    sta G + SHIP_BANK + 1
    lda #<coop_settle_ship
    ldy #>coop_settle_ship
    cpx #0
    beq .Lupdate_action
    lda #<coop_fly_ship
    ldy #>coop_fly_ship
.Lupdate_action:
    sta __rc4
    sty __rc5
    lda G + DEVICES
    cmp #$10
    lda #0
    rol
    asl
    ora #1
    jsr coop_each
    ; Enemy fire alternates between the flying ships each tick.
    ldx #0
    inc G + AIM_TURN
    jsr .Lreserve_alive
    bcc .Laim_copy
    lda G + AIM_TURN
    lsr
    bcs .Laim_second
    lda G + LIVES
    beq .Laim_second
    lda G + RESPAWN
    beq .Laim_copy
.Laim_second:
    ldx #RESERVE
.Laim_copy:
    lda G + PLAYER_X,x
    sta G + AIM
    lda G + PLAYER_X + 1,x
    sta G + AIM + 1
    lda G + PLAYER_Y,x
    sta G + AIM + 2
    lda G + PLAYER_Y + 1,x
    sta G + AIM + 3
    ; view = the first ship; the second when only it flies; else the mean.
    ldx #3
.Lview_first:
    lda G + PLAYER_X,x
    sta G + VIEW,x
    dex
    bpl .Lview_first
    lda G + DEVICES
    cmp #$10
    bcc .Lview_done
    lda G + RESERVE + LIVES
    beq .Lview_done
    lda G + RESERVE + RESPAWN
    bne .Lview_done
    lda G + LIVES
    beq .Lview_second
    lda G + RESPAWN
    bne .Lview_second
    ldx #0
    jsr .Lview_mean
    ldx #2
.Lview_mean:
    clc
    lda G + VIEW,x
    adc G + RESERVE + PLAYER_X,x
    sta __rc6
    lda G + VIEW + 1,x
    adc G + RESERVE + PLAYER_X + 1,x
    cmp #$80
    ror
    sta G + VIEW + 1,x
    lda __rc6
    ror
    sta G + VIEW,x
.Lview_done:
    rts
.Lview_second:
    ldx #3
.Lview_copy:
    lda G + RESERVE + PLAYER_X,x
    sta G + VIEW,x
    dex
    bpl .Lview_copy
    rts
.size coop_update_ships, .-coop_update_ships

; void coop_boost_ship(Game *): Game::tick_boost for the front ship. Outside
; flight, or while a co-op ship is downed or retired, the afterburner is off.
.globl coop_boost_ship
.type coop_boost_ship,@function
coop_boost_ship:
    lda G + LIVES
    beq .Lboost_grounded
    lda G + RESPAWN
    bne .Lboost_grounded
    lda G + MODE
    cmp #MODE_PLAYING
    beq .Lboost_flying
    cmp #MODE_TRANSITION
    bne .Lboost_grounded
.Lboost_flying:
    ; boost_cycle = boost_cycle.saturating_sub(1)
    lda G + BOOST_CYCLE
    ora G + BOOST_CYCLE + 1
    beq .Lboost_ready
    lda G + BOOST_CYCLE
    bne .Lboost_low
    dec G + BOOST_CYCLE + 1
.Lboost_low:
    dec G + BOOST_CYCLE
    lda G + BOOST_CYCLE
    ora G + BOOST_CYCLE + 1
    bne .Lboost_flags
.Lboost_ready:
    lda G + C_FLAGS
    and #$40
    beq .Lboost_flags
    lda #<BOOST_FULL
    sta G + BOOST_CYCLE
    lda #>BOOST_FULL
    sta G + BOOST_CYCLE + 1
    lda #<G
    sta __rc2
    lda #>G
    sta __rc3
    lda #EFFECT_ROLL
    jsr game_sound_player
.Lboost_flags:
    lda G + C_FLAGS
    and #$3f
    ldx G + BOOST_CYCLE + 1
    bne .Lboost_active
    ldx G + BOOST_CYCLE
    cpx #BOOST_COOLDOWN + 1
    bcc .Lboost_store
.Lboost_active:
    ora #$80
.Lboost_store:
    sta G + C_FLAGS
    rts
.Lboost_grounded:
    lda #0
    sta G + BOOST_CYCLE
    sta G + BOOST_CYCLE + 1
    lda G + C_FLAGS
    and #$3f
    sta G + C_FLAGS
    rts
.size coop_boost_ship, .-coop_boost_ship

; void coop_collect_ship(Game *): award pickup HIT_AMOUNT (1 shield, else
; bomb) and 50 points (two score units) to the front ship.
.globl coop_collect_ship
.type coop_collect_ship,@function
coop_collect_ship:
    lda G + HIT_AMOUNT
    cmp #1
    bne .Lcollect_bomb
    lda G + SHIELD
    clc
    adc #60
    cmp #101
    bcc .Lcollect_shield
    lda #100
.Lcollect_shield:
    sta G + SHIELD
    jmp .Lcollect_score
.Lcollect_bomb:
    lda G + BOMBS
    cmp #3
    bcs .Lcollect_score
    inc G + BOMBS
.Lcollect_score:
    lda G + SCORE
    clc
    adc #2
    sta G + SCORE
    lda G + SCORE + 1
    adc #0
    bcc .Lcollect_high
    lda #$ff
    sta G + SCORE
.Lcollect_high:
    sta G + SCORE + 1
    rts
.size coop_collect_ship, .-coop_collect_ship

; uint8_t coop_ship_down(Game *): Game::ship_down. Returns 1 when the partner
; keeps flying (this ship returns after a delay or retires); otherwise 0 (both
; wait for the checkpoint) or 2 (neither ship has a life left).
.globl coop_ship_down
.type coop_ship_down,@function
coop_ship_down:
    lda G + LIVES
    bne .Ldown_seat
    lda G + SWAPPED
    beq .Ldown_seat
    lda G + DEVICES
    and #15
    sta G + DEVICES
.Ldown_seat:
    jsr .Lreserve_alive
    bcs .Ldown_partner
    jsr .Lreserve_present
    bcc .Ldown_alone
    lda #75
    sta G + RESERVE + RESPAWN
    lda G + RESERVE + LIVES
    bne .Ldown_pause
.Ldown_alone:
    lda G + LIVES
    bne .Ldown_pause
    lda #2
    rts
.Ldown_pause:
    lda #0
    rts
.Ldown_partner:
    lda G + LIVES
    beq .Ldown_continue
    lda #75
    sta G + RESPAWN
.Ldown_continue:
    lda #1
    rts
.size coop_ship_down, .-coop_ship_down

; const Vec3 *coop_target(uint8_t select) (address in A/X): the ship an
; enemy selected by `select` pursues: the second for odd selections, or when
; the first is down; otherwise the first.
.globl coop_target
.type coop_target,@function
coop_target:
    tax
    jsr .Lreserve_alive
    bcc .Ltarget_first
    txa
    lsr
    bcs .Ltarget_second
    lda G + LIVES
    beq .Ltarget_second
    lda G + RESPAWN
    bne .Ltarget_second
.Ltarget_first:
    lda #<(G + PLAYER_X)
    ldx #>(G + PLAYER_X)
    rts
.Ltarget_second:
    lda #<(G + RESERVE + PLAYER_X)
    ldx #>(G + RESERVE + PLAYER_X)
    rts
.size coop_target, .-coop_target

; void coop_award_lives(Game *, uint8_t cap): +1 life (up to cap) for each
; ship that still has lives.
.globl coop_award_lives
.type coop_award_lives,@function
coop_award_lives:
    sta __rc6
    ldx #0
    jsr .Laward
    jsr .Lreserve_present
    bcc .Laward_done
    ldx #RESERVE
.Laward:
    lda G + LIVES,x
    beq .Laward_done
    cmp __rc6
    bcs .Laward_done
    inc G + LIVES,x
.Laward_done:
    rts
.size coop_award_lives, .-coop_award_lives

; void coop_refill_ships(Game *): a boss refill for the front ship and any
; seated reserve ship with lives.
.globl coop_refill_ships
.type coop_refill_ships,@function
coop_refill_ships:
    ldx #0
    jsr .Lrefill
    jsr .Lreserve_present
    bcc .Lrefill_done
    lda G + RESERVE + LIVES
    beq .Lrefill_done
    ldx #RESERVE
.Lrefill:
    lda #100
    sta G + SHIELD,x
    lda #3
    sta G + BOMBS,x
.Lrefill_done:
    rts
.size coop_refill_ships, .-coop_refill_ships

; void coop_hit_ships(Game *, const Vec3 *pos, uint8_t ships, uint8_t amount)
; C ABI: game rc2/3, pos rc4/5, ships in A, amount in X (bit 7: a rolling
; ship deflects the projectile at pos). Game::hit_ships through coop_each;
; pos is copied to HIT_AT before rc4/5 takes the action.
.globl coop_hit_ships
.type coop_hit_ships,@function
coop_hit_ships:
    pha
    stx G + HIT_AMOUNT
    ldy #5
.Lhit_position:
    lda (__rc4),y
    sta G + HIT_AT,y
    dey
    bpl .Lhit_position
    lda #<coop_hit_ship
    sta __rc4
    lda #>coop_hit_ship
    sta __rc5
    pla
    jmp coop_each
.size coop_hit_ships, .-coop_hit_ships

; void coop_hit_ship(Game *): damage the front ship, or let its roll deflect.
.globl coop_hit_ship
.type coop_hit_ship,@function
coop_hit_ship:
    lda G + HIT_AMOUNT
    bpl .Lhit_damage
    lda G + ROLL_TICKS
    beq .Lhit_damage
    jmp game_explode_hit
.Lhit_damage:
    lda G + HIT_AMOUNT
    and #$7f
    jmp game_damage_player
.size coop_hit_ship, .-coop_hit_ship

; ------------------------------------------------------------- devices
; The sample (port 2, port 1, keyboard): bits[3], pressed[3].
.Lsample:
    .space 6
; The first seat's device index and the plan being built.
.Lone:
    .space 1
.Lheld:
    .space 1

; void coop_join_ship(Game *): seat a fresh ship in front with the starting
; shield, bombs and lives (Game::join_ship).
.globl coop_join_ship
.type coop_join_ship,@function
coop_join_ship:
    lda G + RESERVE + C_NOW
    pha
    lda G + RESERVE + C_NOW + 1
    pha
    lda #0
    ldx #SHIP_SIZE - 1
.Ljoin_clear:
    sta G,x
    dex
    bpl .Ljoin_clear
    pla
    sta G + C_NOW + 1
    sta G + C_RELEASED + 1
    pla
    sta G + C_NOW
    sta G + C_RELEASED
    lda G + SWAPPED
    beq .Ljoin_lane
    lda #SECOND_SHIP_X
.Ljoin_lane:
    sta G + PLAYER_X
    lda #PLAYER_Z_VALUE
    sta G + PLAYER_Z
    lda #100
    sta G + SHIELD
    lda #3
    sta G + BOMBS
    lda #50
    sta G + INVULN
    lda G + START_LIVES
    sta G + LIVES
    rts
.size coop_join_ship, .-coop_join_ship

; Carry set in Playing or Transition (seats may change).
.Ljoinable:
    lda G + MODE
    cmp #MODE_PLAYING
    beq .Ljoinable_yes
    cmp #MODE_TRANSITION
    beq .Ljoinable_yes
    clc
    rts
.Ljoinable_yes:
    sec
    rts

; uint8_t coop_route(Game *, const DeviceSample *sample)
; C ABI: game rc2/3, sample rc4/5. The decisions of Game::sample_devices; the
; caller feeds the ships the samples staged in STAGED (two InputSamples:
; ms, held bits, presses). In menus the devices act as one controller and
; a fire press takes the first seat. In flight each seat reads its device; an
; unseated keyboard's Left Shift boosts the first. Returns bit 0: a retired
; first player returns; bit 1: feed the second seat; bit 2: it was just taken
; by another device's fire press (seat a fresh ship first; no presses fed).
.globl coop_route
.type coop_route,@function
coop_route:
    ldy #5
.Lroute_copy:
    lda (__rc4),y
    sta .Lsample,y
    dey
    bpl .Lroute_copy
    ldy #6
    lda (__rc4),y
    sta G + STAGED
    sta G + STAGED + 4
    iny
    lda (__rc4),y
    sta G + STAGED + 1
    sta G + STAGED + 5
    lda #0
    sta .Lheld
    lda G + MODE
    cmp #MODE_RESPAWNING
    beq .Lroute_flying
    jsr .Ljoinable
    bcs .Lroute_flying
    ldx #2
.Lroute_menu:
    lda .Lsample + 3,x
    and #FIRE
    beq .Lroute_menu_next
    inx
    stx G + DEVICES
    dex
.Lroute_menu_next:
    dex
    bpl .Lroute_menu
    jmp .Lroute_merged
.Lroute_flying:
    lda G + DEVICES
    and #15
    bne .Lroute_seated
    jmp .Lroute_merged
.Lroute_seated:
    tax
    dex
    stx .Lone
    ; The first seat, with Left Shift from an unseated keyboard.
    lda .Lsample,x
    sta G + STAGED + 2
    lda .Lsample + 3,x
    sta G + STAGED + 3
    lda G + DEVICES
    and #15
    cmp #DEVICE_KEYS
    beq .Lroute_rejoin
    lda G + DEVICES
    and #$f0
    cmp #DEVICE_KEYS << 4
    beq .Lroute_rejoin
    lda .Lsample + 2
    and #BOOST_BIT
    ora G + STAGED + 2
    sta G + STAGED + 2
    lda .Lsample + 5
    and #BOOST_BIT
    ora G + STAGED + 3
    sta G + STAGED + 3
.Lroute_rejoin:
    jsr .Ljoinable
    bcc .Lroute_second
    lda G + LIVES
    bne .Lroute_second
    ldx .Lone
    lda .Lsample + 3,x
    and #FIRE
    beq .Lroute_second
    inc .Lheld
.Lroute_second:
    lda G + DEVICES
    lsr
    lsr
    lsr
    lsr
    beq .Lroute_join
    tax
    dex
    lda .Lsample,x
    sta G + STAGED + 6
    lda .Lsample + 3,x
    sta G + STAGED + 7
    lda #2
    ora .Lheld
    rts
.Lroute_join:
    jsr .Ljoinable
    bcc .Lroute_done
    ldx #0
.Lroute_seat:
    lda .Lsample + 3,x
    and #FIRE
    beq .Lroute_seat_next
    cpx .Lone
    beq .Lroute_seat_next
    txa
    clc
    adc #1
    asl
    asl
    asl
    asl
    ora G + DEVICES
    sta G + DEVICES
    lda .Lsample,x
    sta G + STAGED + 6
    lda #0
    sta G + STAGED + 7
    lda #6
    ora .Lheld
    rts
.Lroute_seat_next:
    inx
    cpx #3
    bne .Lroute_seat
.Lroute_done:
    lda .Lheld
    rts
.Lroute_merged:
    lda #0
    sta G + STAGED + 2
    sta G + STAGED + 3
    ldx #2
.Lroute_or:
    lda .Lsample,x
    ora G + STAGED + 2
    sta G + STAGED + 2
    lda .Lsample + 3,x
    ora G + STAGED + 3
    sta G + STAGED + 3
    dex
    bpl .Lroute_or
    lda #0
    rts
.size coop_route, .-coop_route

; uint16_t coop_held(const Game *, const uint8_t bits[3]) (game rc2/3 unused,
; bits rc4/5): the first seat's held bits in A (every device's while
; unseated) and the second seat's in X.
.globl coop_held
.type coop_held,@function
coop_held:
    ldx #0
    lda G + DEVICES
    lsr
    lsr
    lsr
    lsr
    beq .Lheld_first
    tay
    dey
    lda (__rc4),y
    tax
.Lheld_first:
    lda G + DEVICES
    and #15
    beq .Lheld_any
    tay
    dey
    lda (__rc4),y
    rts
.Lheld_any:
    ldy #2
    lda (__rc4),y
    dey
    ora (__rc4),y
    dey
    ora (__rc4),y
    rts
.size coop_held, .-coop_held
