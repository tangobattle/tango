use prost::Message;

fn rom_family_and_variant(rom: &str) -> Option<(&str, u32)> {
    Some(match rom {
        "MEGAMAN6_GXXBR5E" => ("bn6", 0),
        "MEGAMAN6_FXXBR6E" => ("bn6", 1),
        "ROCKEXE6_GXXBR5J" => ("exe6", 0),
        "ROCKEXE6_RXXBR6J" => ("exe6", 1),
        "MEGAMAN5_TP_BRBE" => ("bn5", 0),
        "MEGAMAN5_TC_BRKE" => ("bn5", 1),
        "ROCKEXE5_TOBBRBJ" => ("exe5", 0),
        "ROCKEXE5_TOCBRKJ" => ("exe5", 1),
        "ROCKEXE4.5ROBR4J" => ("exe45", 0),
        "MEGAMANBN4RSB4WE" => ("bn4", 0),
        "MEGAMANBN4BMB4BE" => ("bn4", 1),
        "ROCK_EXE4_RSB4WJ" => ("exe4", 0),
        "ROCK_EXE4_BMB4BJ" => ("exe4", 1),
        "MEGA_EXE3_WHA6BE" => ("bn3", 0),
        "MEGA_EXE3_BLA3XE" => ("bn3", 1),
        "ROCKMAN_EXE3A6BJ" => ("exe3", 0),
        "ROCK_EXE3_BKA3XJ" => ("exe3", 1),
        "MEGAMAN_EXE2AE2E" => ("bn2", 0),
        "ROCKMAN_EXE2AE2J" => ("exe2", 0),
        "MEGAMAN_BN\0\0AREE" => ("bn1", 0),
        "ROCKMAN_EXE\0AREJ" => ("exe1", 0),
        _ => {
            return None;
        }
    })
}

fn convert_side(v10: &super::protos::replay10::metadata::Side) -> Result<super::metadata::Side, std::io::Error> {
    Ok(super::metadata::Side {
        nickname: v10.nickname.clone(),
        reveal_setup: v10.reveal_setup,
        game_info: v10
            .game_info
            .as_ref()
            .map(|gi| {
                let (rom_family, rom_variant) = rom_family_and_variant(&gi.rom).ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unknown rom: {}", gi.rom))
                })?;
                Ok::<_, std::io::Error>(super::metadata::GameInfo {
                    rom_family: rom_family.to_string(),
                    rom_variant,
                    patch: gi.patch.as_ref().map(|patch| super::metadata::game_info::Patch {
                        name: patch.name.clone(),
                        version: patch.version.clone(),
                    }),
                })
            })
            .map_or(Ok(None), |v| v.map(Some))?,
    })
}

pub fn decode_metadata(raw: &[u8]) -> Result<super::Metadata, std::io::Error> {
    let metadata = super::protos::replay10::Metadata::decode(raw)?;

    Ok(super::Metadata {
        ts: metadata.ts,
        link_code: metadata.link_code,
        local_side: metadata
            .local_side
            .map(|side| convert_side(&side))
            .map_or(Ok(None), |v| v.map(Some))?,
        remote_side: metadata
            .remote_side
            .map(|side| convert_side(&side))
            .map_or(Ok(None), |v| v.map(Some))?,
        round: 0,      // Impossible to tell.
        match_type: 0, // Impossible to tell.
        match_subtype: 0,
    })
}
