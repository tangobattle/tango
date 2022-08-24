use fluent_templates::Loader;

use crate::i18n;

pub struct About {
    emblem: egui_extras::RetainedImage,
}

impl About {
    pub fn new() -> Self {
        About {
            emblem: egui_extras::RetainedImage::from_image_bytes(
                "emblem",
                include_bytes!("../emblem.png"),
            )
            .unwrap(),
        }
    }

    pub fn show(
        &self,
        ctx: &egui::Context,
        lang: &unic_langid::LanguageIdentifier,
        open: &mut bool,
    ) {
        egui::Window::new(format!(
            "❓ {}",
            i18n::LOCALES.lookup(lang, "about").unwrap()
        ))
        .id(egui::Id::new("about-window"))
        .default_width(320.0)
        .open(open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.heading(format!(
                        "Tango v{}-{}",
                        env!("CARGO_PKG_VERSION"),
                        git_version::git_version!(),
                    ));

                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        self.emblem.show_scaled(ui, 0.5);
                    });
                    ui.add_space(8.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.hyperlink_to("Tango", "https://tangobattle.com");
                        ui.label(" would not be a reality without the work of the many people who have helped make this possible.",);
                    });

                    ui.heading("Development");
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(" • ");
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Emulation: ");
                                ui.hyperlink_to("endrift", "https://twitter.com/endrift");
                                ui.label(" (mGBA)");
                            });
                        });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(" • ");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label("Reverse engineering: ");

                                ui.hyperlink_to("pnw_ssbmars", "https://twitter.com/pnw_ssbmars");
                                ui.label(" (BN3)");

                                ui.label(", ");

                                ui.hyperlink_to("XKirby", "https://github.com/XKirby");
                                ui.label(" (BN3)");

                                ui.label(", ");

                                ui.hyperlink_to("luckytyphlosion", "https://github.com/luckytyphlosion");
                                ui.label(" (BN6)");

                                ui.label(", ");

                                ui.hyperlink_to("LanHikari22", "https://github.com/LanHikari22");
                                ui.label(" (BN6)");

                                ui.label(", ");

                                ui.hyperlink_to("GreigaMaster", "https://twitter.com/GreigaMaster");
                                ui.label(" (BN)");

                                ui.label(", ");

                                ui.hyperlink_to("Prof. 9", "https://twitter.com/Prof9");
                                ui.label(" (BN)");

                                ui.label(", ");

                                ui.hyperlink_to("National Security Agency", "https://www.nsa.gov");
                                ui.label(" (Ghidra)");

                                ui.label(", ");


                                ui.hyperlink_to("aldelaro5", "https://twitter.com/aldelaro5");
                                ui.label(" (Ghidra)");
                            });
                        });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(" • ");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label("Porting: ");

                                ui.hyperlink_to("ubergeek77", "https://github.com/ubergeek77");
                                ui.label(" (Linux)");

                                ui.label(", ");

                                ui.hyperlink_to("Akatsuki", "https://github.com/Akatsuki");
                                ui.label(" (macOS)");
                            });
                        });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(" • ");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label("Game support: ");

                                ui.hyperlink_to("weenie", "https://github.com/bigfarts");
                                ui.label(" (BN)");

                                ui.label(", ");

                                ui.hyperlink_to("GreigaMaster", "https://twitter.com/GreigaMaster");
                                ui.label(" (EXE4.5)");
                            });
                        });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(" • ");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label("Odds and ends: ");

                                ui.hyperlink_to("sailormoon", "https://github.com/sailormoon");

                                ui.label(", ");

                                ui.hyperlink_to("Shiz", "https://twitter.com/dev_console");

                                ui.label(", ");

                                ui.hyperlink_to("Karate_Bugman", "https://twitter.com/Karate_Bugman");
                            });
                        });
                    });

                    ui.heading("Translation");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Japanese: ");

                            ui.hyperlink_to("weenie", "https://github.com/bigfarts");

                            ui.label(", ");

                            ui.hyperlink_to("Nonstopmop", "https://twitter.com/seventhfonist42");

                            ui.label(", ");

                            ui.hyperlink_to("dhenva", "https://twitch.tv/dhenva");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Simplified Chinese: ");

                            ui.hyperlink_to("weenie", "https://github.com/bigfarts");

                            ui.label(", ");

                            ui.hyperlink_to("Hikari Calyx", "https://twitter.com/Hikari_Calyx");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Traditional Chinese: ");

                            ui.hyperlink_to("weenie", "https://github.com/bigfarts");

                            ui.label(", ");

                            ui.hyperlink_to("Hikari Calyx", "https://twitter.com/Hikari_Calyx");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Spanish: ");

                            ui.hyperlink_to("Karate_Bugman", "https://twitter.com/Karate_Bugman");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Brazilian Portuguese: ");

                            ui.hyperlink_to("Darkgaia", "https://discord.gg/hPrFVaaRrU");

                            ui.label(", ");

                            ui.hyperlink_to("mushiguchi", "https://twitter.com/mushiguchi");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("French: ");

                            ui.hyperlink_to("Sheriel Phoenix", "https://twitter.com/Sheriel_Phoenix");

                            ui.label(", ");

                            ui.hyperlink_to("Justplay", "https://twitter.com/justplayfly");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("German: ");

                            ui.hyperlink_to("KenDeep", "https://twitch.tv/kendeep_fgc");
                        });
                    });

                    ui.heading("Art");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Logo: ");

                            ui.hyperlink_to("saladdammit", "https://twitter.com/saladdammit");
                        });
                    });


                    ui.heading("Special thanks");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Playtesting: ");

                            ui.hyperlink_to("N1GP", "https://n1gp.net");
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("#1 fan: ");

                            ui.hyperlink_to("playerzero", "https://twitter.com/Playerzero_exe");
                        });
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("And, of course, a huge thank you to ");
                        ui.hyperlink_to("CAPCOM", "https://www.capcom.com");
                        ui.label(" for making Mega Man Battle Network!");
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Tango is licensed under the terms of the ");
                        ui.hyperlink_to("GNU Affero General Public License v3", "https://tldrlegal.com/license/gnu-affero-general-public-license-v3-(agpl-3.0)");
                        ui.label(". That means you’re free to modify the ");
                        ui.hyperlink_to("source code", "https://github.com/tangobattle");
                        ui.label(", as long as you contribute your changes back!");
                    });
                });
        });
    }
}
