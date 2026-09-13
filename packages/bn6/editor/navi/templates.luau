--!strict
-- Link Navi equip templates; offsets refer to the 0x64-byte NaviStats record.
local offsets: {[string]: number} = {
    a_pwr_atk = 0x39,
    air_shoes = 0x1c,
    attack = 0x01,
    b_left_ability = 0x07,
    b_pwr_atk = 0x05,
    base_max_hp = 0x3e,
    charge = 0x03,
    current_hp = 0x40,
    custom_level = 0x0a,
    effective_max_hp = 0x42,
    float_shoes = 0x1b,
    mega_limit = 0x0b,
    navi_id = 0x29,
    speed = 0x02,
    super_armor = 0x23,
    unk_20 = 0x20,
    unk_48 = 0x48,
}
local profiles: {{[string]: number}} = {
    { -- HeatMan
        navi_id = 1,
        attack = 3,
        speed = 2,
        charge = 2,
        b_pwr_atk = 0x40,
        b_left_ability = 0xff,
        custom_level = 6,
        mega_limit = 6,
        float_shoes = 1,
        a_pwr_atk = 0xff,
        base_max_hp = 800,
        current_hp = 2000,
        effective_max_hp = 2000,
    },
    { -- ElecMan
        navi_id = 2,
        attack = 3,
        speed = 2,
        charge = 2,
        b_pwr_atk = 0x44,
        b_left_ability = 0xff,
        custom_level = 6,
        mega_limit = 7,
        float_shoes = 1,
        a_pwr_atk = 0xff,
        base_max_hp = 800,
        current_hp = 1900,
        effective_max_hp = 1900,
    },
    { -- SlashMan
        navi_id = 3,
        attack = 2,
        speed = 4,
        charge = 1,
        b_pwr_atk = 0x43,
        b_left_ability = 0xff,
        custom_level = 7,
        mega_limit = 6,
        a_pwr_atk = 0xff,
        base_max_hp = 800,
        current_hp = 1800,
        effective_max_hp = 1800,
    },
    { -- EraseMan
        navi_id = 4,
        attack = 3,
        speed = 2,
        charge = 3,
        b_pwr_atk = 0x47,
        b_left_ability = 0xff,
        custom_level = 6,
        mega_limit = 7,
        a_pwr_atk = 0xff,
        base_max_hp = 700,
        current_hp = 1500,
        effective_max_hp = 1500,
    },
    { -- ChargeMan
        navi_id = 5,
        attack = 3,
        speed = 2,
        charge = 4,
        b_pwr_atk = 0x48,
        b_left_ability = 0xff,
        custom_level = 8,
        mega_limit = 6,
        super_armor = 1,
        a_pwr_atk = 0x29,
        base_max_hp = 800,
        current_hp = 1800,
        effective_max_hp = 1800,
    },
    { -- SpoutMan
        navi_id = 6,
        attack = 3,
        speed = 2,
        charge = 2,
        b_pwr_atk = 0x41,
        b_left_ability = 0xff,
        custom_level = 6,
        mega_limit = 6,
        unk_20 = 1,
        a_pwr_atk = 0x20,
        base_max_hp = 800,
        current_hp = 1900,
        effective_max_hp = 1900,
    },
    { -- TomahawkMan
        navi_id = 7,
        attack = 4,
        speed = 2,
        charge = 2,
        b_pwr_atk = 0x45,
        b_left_ability = 0xff,
        custom_level = 6,
        mega_limit = 7,
        unk_20 = 1,
        super_armor = 1,
        a_pwr_atk = 0x1f,
        base_max_hp = 800,
        current_hp = 1800,
        effective_max_hp = 1800,
    },
    { -- TenguMan
        navi_id = 8,
        attack = 3,
        speed = 4,
        charge = 1,
        b_pwr_atk = 0x42,
        b_left_ability = 0x10,
        custom_level = 7,
        mega_limit = 6,
        float_shoes = 1,
        air_shoes = 1,
        unk_20 = 1,
        a_pwr_atk = 0xff,
        base_max_hp = 800,
        current_hp = 1800,
        effective_max_hp = 1800,
    },
    { -- GroundMan
        navi_id = 9,
        attack = 4,
        speed = 2,
        charge = 2,
        b_pwr_atk = 0x4a,
        b_left_ability = 0xff,
        custom_level = 6,
        mega_limit = 7,
        unk_20 = 1,
        super_armor = 1,
        a_pwr_atk = 0xff,
        base_max_hp = 800,
        current_hp = 2000,
        effective_max_hp = 2000,
    },
    { -- DustMan
        navi_id = 10,
        attack = 2,
        speed = 3,
        charge = 2,
        b_pwr_atk = 0x49,
        b_left_ability = 0xff,
        custom_level = 8,
        mega_limit = 6,
        unk_20 = 1,
        super_armor = 1,
        a_pwr_atk = 0xff,
        base_max_hp = 800,
        current_hp = 2000,
        effective_max_hp = 2000,
    },
    { -- ProtoMan
        navi_id = 11,
        attack = 4,
        speed = 3,
        charge = 3,
        b_pwr_atk = 0x32,
        b_left_ability = 0x30,
        custom_level = 7,
        mega_limit = 6,
        a_pwr_atk = 0x2d,
        unk_48 = 0x32,
        base_max_hp = 800,
        current_hp = 1400,
        effective_max_hp = 1400,
    },
}

local function stats(id: number): buffer?
    if id < 1 or id > #profiles or id ~= math.floor(id) then return nil end
    local bytes = buffer.create(0x64)
    buffer.writeu8(bytes, 0x00, 0x08) -- unk_00
    buffer.writeu8(bytes, 0x09, 50) -- reg_memory
    buffer.writeu8(bytes, 0x0c, 1) -- giga_limit
    buffer.writeu8(bytes, 0x0e, 0x99) -- mood
    buffer.writeu8(bytes, 0x12, 0xff) -- unk_12
    buffer.writeu8(bytes, 0x21, 3) -- beast_out_counter
    buffer.writeu8(bytes, 0x27, 0x1f) -- unk_27
    buffer.fill(bytes, 0x2e, 0xff, 3) -- regular_chip_indexes
    buffer.writeu8(bytes, 0x46, 1) -- unk_46
    buffer.writeu8(bytes, 0x4a, 0x0a) -- unk_4a
    buffer.fill(bytes, 0x56, 0xff, 6) -- tag_chip_indexes
    for name, value in profiles[id] do
        local offset = offsets[name]
        if string.find(name, "_hp", 1, true) then
            buffer.writeu16(bytes, offset, value)
        else
            buffer.writeu8(bytes, offset, value)
        end
    end
    return bytes
end
return {stats = stats}
