use byteorder::ByteOrder;

use crate::rom;

pub struct Offsets {
    chip_data: u32,
    chip_names_pointers: u32,
    chip_icon_palette_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
}

#[rustfmt::skip]
pub static AE2E_00: Offsets = Offsets {
    chip_data:                      0x0800e450,
    chip_names_pointers:            0x0800b528,
    element_icons_pointer:          0x08025fe0,
    element_icon_palette_pointer:   0x08005388,
    chip_icon_palette_pointer:      0x08025fe0,
};

#[rustfmt::skip]
pub static AE2J_00: Offsets = Offsets {
    chip_data:                      0x0800e430,
    chip_names_pointers:            0x0800b51c,
    element_icons_pointer:          0x0800b884,
    element_icon_palette_pointer:   0x08005384,
    chip_icon_palette_pointer:      0x08025f8c,
};

lazy_static! {
    pub static ref TEXT_PARSE_OPTIONS: rom::text::ParseOptions =
        rom::text::ParseOptions::new(0xe5, 0xe7).with_command(0xe8, 0);
}

pub struct Assets {
    element_icons: [image::RgbaImage; 5],
    chips: [rom::Chip; 315],
}

impl Assets {
    pub fn new(
        offsets: &Offsets,
        charset: &'static [&'static str],
        rom: &[u8],
        wram: &[u8],
    ) -> Self {
        let mapper = rom::MemoryMapper::new(rom, wram);

        let chip_icon_palette = rom::read_palette(
            &mapper.get(
                (byteorder::LittleEndian::read_u32(
                    &mapper.get(offsets.chip_icon_palette_pointer)[..4],
                )),
            )[..32],
        );

        Self {
            element_icons: {
                let palette = rom::read_palette(
                    &mapper.get(
                        (byteorder::LittleEndian::read_u32(
                            &mapper.get(offsets.element_icon_palette_pointer)[..4],
                        )),
                    )[..32],
                );
                {
                    let buf = mapper.get(
                        (byteorder::LittleEndian::read_u32(
                            &mapper.get(offsets.element_icons_pointer)[..4],
                        )),
                    );
                    (0..5)
                        .map(|i| {
                            rom::apply_palette(
                                rom::read_merged_tiles(
                                    &buf[i * rom::TILE_BYTES * 4..(i + 1) * rom::TILE_BYTES * 4],
                                    2,
                                )
                                .unwrap(),
                                &palette,
                            )
                        })
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap()
                }
            },
            chips: (0..315)
                .map(|i| {
                    let buf = &mapper.get(offsets.chip_data)[i * 0x20..(i + 1) * 0x20];
                    rom::Chip {
                        name: {
                            let (i, pointer) = if i < 0x100 {
                                (i, offsets.chip_names_pointers)
                            } else {
                                (i - 0x100, offsets.chip_names_pointers + 4)
                            };
                            if let Ok(parts) = rom::text::parse_entry(
                                &mapper.get(byteorder::LittleEndian::read_u32(
                                    &mapper.get(pointer)[..4],
                                )),
                                i,
                                &TEXT_PARSE_OPTIONS,
                            ) {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            rom::text::Part::Literal(c) => {
                                                charset.get(c).unwrap_or(&"�")
                                            }
                                            _ => "",
                                        }
                                        .chars()
                                    })
                                    .collect::<String>()
                            } else {
                                "???".to_string()
                            }
                        },
                        icon: rom::apply_palette(
                            rom::read_merged_tiles(
                                &mapper
                                    .get(byteorder::LittleEndian::read_u32(&buf[0x14..0x14 + 4]))
                                    [..rom::TILE_BYTES * 4],
                                2,
                            )
                            .unwrap(),
                            &chip_icon_palette,
                        ),
                        codes: buf[0x00..0x06].iter().filter(|&code| *code != 0xff).cloned().collect(),
                        element: buf[0x06] as usize,
                        class: rom::ChipClass::Standard,
                        dark: false,
                        mb: buf[0x0a],
                        damage: byteorder::LittleEndian::read_u16(&buf[0x0c..0x0c + 2]) as u32,
                    }
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        }
    }
}

impl rom::Assets for Assets {
    fn chip(&self, id: usize) -> Option<&rom::Chip> {
        self.chips.get(id)
    }

    fn element_icon(&self, id: usize) -> Option<&image::RgbaImage> {
        self.element_icons.get(id)
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &'static [&'static str] = &[" ","0","1","2","3","4","5","6","7","8","9","a","b","c","d","e","f","g","h","i","j","k","l","m","n","o","p","q","r","s","t","u","v","w","x","y","z","A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P","Q","R","S","T","U","V","W","X","Y","Z","V2","V3","-","×","=",":","?","+","÷","※","*","!","�","%","&",",","。",".","・",";","'","\"","~","/","(",")","「","」","↑","→","↓","←","@","★","♪","<",">","[bracket1]","[bracket2]","■","$","#"];

#[rustfmt::skip]
pub const JA_CHARSET: &'static [&'static str] = &[" ","0","1","2","3","4","5","6","7","8","9","ア","イ","ウ","エ","オ","カ","キ","ク","ケ","コ","サ","シ","ス","セ","ソ","タ","チ","ツ","テ","ト","ナ","ニ","ヌ","ネ","ノ","ハ","ヒ","フ","ヘ","ホ","マ","ミ","ム","メ","モ","ヤ","ユ","ヨ","ラ","リ","ル","レ","ロ","ワ","V2","V3","ヲ","ン","ガ","ギ","グ","ゲ","ゴ","ザ","ジ","ズ","ゼ","ゾ","ダ","ヂ","ヅ","デ","ド","バ","ビ","ブ","ベ","ボ","パ","ピ","プ","ペ","ポ","ァ","ィ","ゥ","ェ","ォ","ッ","ャ","ュ","ョ","ヴ","A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P","Q","R","S","T","U","V","W","X","Y","Z","ー","×","=",":","?","+","÷","※","*","!","[?]","%","&","、","。",".","・",";","'","\"","~","/","(",")","「","」","↑","→","↓","←","@","♥","♪","あ","い","う","え","お","か","き","く","け","こ","さ","し","す","せ","そ","た","ち","つ","て","と","な","に","ぬ","ね","の","は","ひ","ふ","へ","ほ","ま","み","む","め","も","や","ゆ","よ","ら","り","る","れ","ろ","わ","ゐ","ゑ","を","ん","が","ぎ","ぐ","げ","ご","ざ","じ","ず","ぜ","ぞ","だ","ぢ","づ","で","ど","ば","び","ぶ","べ","ぼ","ぱ","ぴ","ぷ","ぺ","ぽ","ぁ","ぃ","ぅ","ぇ","ぉ","っ","ゃ","ゅ","ょ","a","b","c","d","e","f","g","h","i","j","k","l","m","n","o","p","q","r","s","t","u","v","w","x","y","z","容","量","ヰ","ヱ","止","彩","起","父","博","士","一","二","三","四","五","六","七","八","九","十","百","千","万","脳","上","下","左","右","手","足","日","目","月","磁","真","人","入","出","山","口","光","電","気","話","広","王","名","前","学","校","室","世","界","機","器","大","小","中","自","分","間","問","門","熱","斗","要","道","行","街","屋","水","見","教","走","先","生","長","今","点","女","子","言","会","来","風","吹","速","思","時","円","知","毎","年","火","朝","計","画","休","体","波","回","外","多","正","死","値","合","戦","争","秋","原","町","天","用","金","男","作","数","方","社","攻","撃","力","同","武","何","発","少","以","早","暮","面","組","後","文","字","本","階","岩","才","者","立","々","ヶ","連","射","国","耳","土","炎","伊","集","院","各","科","省","祐","朗","枚","川","花","兄",];
