use eframe::egui;
use egui_plot::{CoordinatesFormatter, Corner, Line, Plot, PlotPoints, VLine};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Simulateur de latence du pipeline")
            .with_inner_size(egui::vec2(900.0, 600.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Simulateur de latence du pipeline",
        options,
        Box::new(|_cc| Ok(Box::new(AppState::default()))),
    )
}

#[derive(Debug)]
struct PartitionParams {
    /// Coût fixe par paquet en microsecondes.
    fixed_us: f32,
    /// Coût par octet en nanosecondes.
    per_byte_ns: f32,
}

impl Default for PartitionParams {
    fn default() -> Self {
        Self {
            fixed_us: 5.0,
            per_byte_ns: 5.0,
        }
    }
}

struct AppState {
    /// Débit global à traiter (en Mbps).
    throughput_mbps: f32,
    /// Taille moyenne du paquet (en octets).
    packet_size_bytes: f32,
    /// Limite de latence cible en microsecondes (1 ms = 1000 µs).
    latency_budget_us: f32,
    /// G/G/1 : carré du coefficient de variation des inter-arrivées (c_a²). Poisson = 1.
    ca_sq: f32,
    /// G/G/1 : carré du coefficient de variation du temps de service (c_s²). Déterministe = 0.
    cs_sq: f32,
    /// Paramètres des 3 partitions.
    p1: PartitionParams,
    p2: PartitionParams,
    p3: PartitionParams,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            throughput_mbps: 40.0,
            packet_size_bytes: 600.0,
            latency_budget_us: 1000.0,
            ca_sq: 1.0,
            cs_sq: 0.0,
            p1: PartitionParams {
                fixed_us: 20.0,
                per_byte_ns: 2.0,
            },
            p2: PartitionParams {
                fixed_us: 80.0,
                per_byte_ns: 8.0,
            },
            p3: PartitionParams {
                fixed_us: 20.0,
                per_byte_ns: 2.0,
            },
        }
    }
}

impl AppState {
    fn part_latency_us(&self, p: &PartitionParams, size_bytes: f32) -> f32 {
        let s = size_bytes.max(1.0);
        p.fixed_us + (p.per_byte_ns * s) / 1000.0
    }

    /// Temps de traitement seul (sans attente en file) pour la taille de paquet courante.
    fn total_latency_us(&self) -> f32 {
        let size = self.packet_size_bytes.max(1.0);
        self.part_latency_us(&self.p1, size)
            + self.part_latency_us(&self.p2, size)
            + self.part_latency_us(&self.p3, size)
    }

    /// Temps par étage (µs) pour la taille de paquet courante: [P1, P2, P3].
    fn stage_times_us(&self, size_bytes: f32) -> [f32; 3] {
        let s = size_bytes.max(1.0);
        [
            self.part_latency_us(&self.p1, s),
            self.part_latency_us(&self.p2, s),
            self.part_latency_us(&self.p3, s),
        ]
    }

    /// Goulot d'étranglement (max des trois étages). Le débit max = 1/bottleneck_us paquets/s.
    fn bottleneck_us(&self, size_bytes: f32) -> f32 {
        let [p1, p2, p3] = self.stage_times_us(size_bytes);
        p1.max(p2).max(p3)
    }

    /// Latence avec files entre étages (modèle G/G/1, approximation de Kingman).
    /// E[T] = E[S] * (1 + (ρ/(1-ρ)) * (c_a² + c_s²)/2).
    fn total_latency_with_queuing_us(&self, size_bytes: f32, inter_arrival_us: f32) -> f32 {
        let [p1, p2, p3] = self.stage_times_us(size_bytes);
        let ia = inter_arrival_us.max(0.1);
        let cv = (self.ca_sq + self.cs_sq) * 0.5;
        let sojourn = |pk: f32| -> f32 {
            let rho = pk / ia;
            if rho >= 1.0 {
                return f32::INFINITY;
            }
            pk * (1.0 + (rho / (1.0 - rho)) * cv)
        };
        sojourn(p1) + sojourn(p2) + sojourn(p3)
    }

    fn packets_per_second(&self) -> f32 {
        let bits_per_second = self.throughput_mbps.max(0.1) * 1_000_000.0;
        let bits_per_packet = self.packet_size_bytes.max(1.0) * 8.0;
        bits_per_second / bits_per_packet
    }

    fn inter_arrival_us(&self) -> f32 {
        let pps = self.packets_per_second();
        if pps <= 0.0 {
            return f32::INFINITY;
        }
        1_000_000.0 / pps
    }

    /// Inter-arrivée (µs) si tous les paquets avaient la même taille `size_bytes`.
    fn inter_arrival_us_for_size(&self, size_bytes: f32) -> f32 {
        let bps = self.throughput_mbps.max(0.1) * 1_000_000.0;
        let bits_per_packet = size_bytes.max(1.0) * 8.0;
        let pps = bps / bits_per_packet;
        if pps <= 0.0 {
            return f32::INFINITY;
        }
        1_000_000.0 / pps
    }

    /// Taille moyenne des files entre partitions (en paquets), modèle G/G/1.
    /// Lq = ρ² * (c_a² + c_s²) / (2*(1-ρ)). Retourne (file P1→P2, file P2→P3).
    fn queue_sizes_avg(&self, size_bytes: f32, inter_arrival_us: f32) -> (f32, f32) {
        let [_p1, p2, p3] = self.stage_times_us(size_bytes);
        let ia = inter_arrival_us.max(0.1);
        let cv = (self.ca_sq + self.cs_sq) * 0.5;
        let lq = |pk: f32| -> f32 {
            let rho = pk / ia;
            if rho >= 1.0 {
                return f32::INFINITY;
            }
            (rho * rho) * cv / (1.0 - rho)
        };
        (lq(p2), lq(p3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_total_latency_simple() {
        let mut app = AppState::default();
        app.packet_size_bytes = 1000.0;

        let lat = app.total_latency_us();
        assert!(lat > 0.0);
    }

    #[test]
    fn test_packets_per_second_and_inter_arrival() {
        let mut app = AppState::default();
        app.throughput_mbps = 16.0;
        app.packet_size_bytes = 1000.0;

        let pps = app.packets_per_second();
        assert!(pps > 0.0);

        let inter = app.inter_arrival_us();
        assert!(inter.is_finite());
        assert!(inter > 0.0);
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let inter_arrival = self.inter_arrival_us();
        let bottleneck = self.bottleneck_us(self.packet_size_bytes);
        let total_no_queue = self.total_latency_us();
        let total_with_queue = self.total_latency_with_queuing_us(
            self.packet_size_bytes,
            inter_arrival,
        );
        let latency_for_budget = if total_with_queue.is_finite() {
            total_with_queue
        } else {
            total_no_queue
        };
        let over_budget = latency_for_budget > self.latency_budget_us;
        let unstable = inter_arrival < bottleneck;

        let (status_text, status_color, status_fill) = if over_budget || unstable {
            (
                "NE RESPECTE PAS LA CONTRAINTE",
                egui::Color32::WHITE,
                egui::Color32::from_rgb(180, 40, 40),
            )
        } else {
            (
                "OK — PIPELINE STABLE",
                egui::Color32::BLACK,
                egui::Color32::from_rgb(40, 160, 40),
            )
        };

        // Barre de statut en haut, indépendante du reste
        egui::TopBottomPanel::top("status_bar").show(ctx, |ui| {
            let status_frame = egui::Frame::none()
                .fill(status_fill)
                .inner_margin(egui::Margin::same(10.0));
            status_frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(status_text)
                            .size(18.0)
                            .color(status_color),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "Latence (avec files): {:.0} µs",
                            if total_with_queue.is_finite() {
                                total_with_queue
                            } else {
                                total_no_queue
                            }
                        ))
                        .color(status_color),
                    );
                    ui.label(
                        egui::RichText::new(format!("Budget: {:.0} µs", self.latency_budget_us))
                            .color(status_color),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Ratio: {:.2}",
                            latency_for_budget / self.latency_budget_us.max(1.0)
                        ))
                        .color(status_color),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Goulot: {:.0} µs",
                            bottleneck
                        ))
                        .color(status_color),
                    );
                });
            });
        });

        // Panneau latéral gauche uniquement pour les paramètres
        egui::SidePanel::left("sliders_panel")
            .resizable(false)
            .exact_width(320.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Paramètres globaux");
                    ui.add(
                        egui::Slider::new(&mut self.throughput_mbps, 1.0..=100.0)
                            .text("Débit (Mbps)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.packet_size_bytes, 64.0..=1500.0)
                            .text("Taille paquet (octets)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.latency_budget_us, 100.0..=5000.0)
                            .text("Budget latence (µs)"),
                    );

                    ui.separator();
                    ui.heading("G/G/1 (variabilité)");
                    ui.label("c_a² = carré C.V. inter-arrivées (Poisson = 1)");
                    ui.add(
                        egui::Slider::new(&mut self.ca_sq, 0.0..=5.0)
                            .text("c_a²"),
                    );
                    ui.label("c_s² = carré C.V. temps de service (Déterministe = 0)");
                    ui.add(
                        egui::Slider::new(&mut self.cs_sq, 0.0..=5.0)
                            .text("c_s²"),
                    );

                    ui.separator();
                    ui.heading("Partition 1");
                    ui.add(
                        egui::Slider::new(&mut self.p1.fixed_us, 0.0..=200.0)
                            .text("Fixe (µs)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.p1.per_byte_ns, 0.0..=50.0)
                            .text("Par octet (ns)"),
                    );

                    ui.separator();
                    ui.heading("Partition 2");
                    ui.add(
                        egui::Slider::new(&mut self.p2.fixed_us, 0.0..=300.0)
                            .text("Fixe (µs)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.p2.per_byte_ns, 0.0..=100.0)
                            .text("Par octet (ns)"),
                    );

                    ui.separator();
                    ui.heading("Partition 3");
                    ui.add(
                        egui::Slider::new(&mut self.p3.fixed_us, 0.0..=200.0)
                            .text("Fixe (µs)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.p3.per_byte_ns, 0.0..=50.0)
                            .text("Par octet (ns)"),
                    );
                });
            });

        // Panneau central uniquement pour pipeline + graphe
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.heading("Pipeline");
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width().min(450.0), 120.0),
                    egui::Sense::hover(),
                );
                let painter = ui.painter();
                let margin = 12.0;
                let pr = rect.shrink(margin);
                let w = pr.width();
                let h = pr.height();
                let y = pr.center().y;
                let wb = w / 4.0;
                let b1 = egui::Rect::from_center_size(
                    egui::pos2(pr.left() + wb * 0.5, y),
                    egui::vec2(wb * 0.7, h * 0.5),
                );
                let b2 = egui::Rect::from_center_size(
                    egui::pos2(pr.left() + wb * 1.7, y),
                    egui::vec2(wb * 0.7, h * 0.5),
                );
                let b3 = egui::Rect::from_center_size(
                    egui::pos2(pr.left() + wb * 2.9, y),
                    egui::vec2(wb * 0.7, h * 0.5),
                );
                let part_lat = |p: &PartitionParams| -> f32 {
                    let s = self.packet_size_bytes.max(1.0);
                    p.fixed_us + (p.per_byte_ns * s) / 1000.0
                };
                let stroke = egui::Stroke::new(2.0, egui::Color32::LIGHT_BLUE);
                let fill = egui::Color32::from_gray(30);
                for (r, label, lat) in [
                    (b1, "P1", part_lat(&self.p1)),
                    (b2, "P2", part_lat(&self.p2)),
                    (b3, "P3", part_lat(&self.p3)),
                ] {
                    painter.rect(r, 4.0, fill, stroke);
                    painter.text(
                        r.center_top() + egui::vec2(0.0, 4.0),
                        egui::Align2::CENTER_TOP,
                        label,
                        egui::FontId::proportional(12.0),
                        egui::Color32::WHITE,
                    );
                    painter.text(
                        r.center_bottom() - egui::vec2(0.0, 4.0),
                        egui::Align2::CENTER_BOTTOM,
                        format!("{:.0} µs", lat),
                        egui::FontId::proportional(11.0),
                        egui::Color32::WHITE,
                    );
                }
                let ac = if over_budget || unstable {
                    egui::Color32::RED
                } else {
                    egui::Color32::from_rgb(10, 180, 10)
                };
                let (q12, q23) = self.queue_sizes_avg(
                    self.packet_size_bytes,
                    self.inter_arrival_us(),
                );
                let fmt_q = |q: f32| -> String {
                    if q.is_finite() {
                        format!("{:.2}", q)
                    } else {
                        "∞".to_string()
                    }
                };
                let mid1 = (b1.right() + b2.left()) / 2.0;
                let mid2 = (b2.right() + b3.left()) / 2.0;
                painter.arrow(
                    egui::pos2(b1.right() + 4.0, y),
                    egui::vec2((b2.left() - 4.0) - (b1.right() + 4.0), 0.0),
                    egui::Stroke::new(2.0, ac),
                );
                painter.text(
                    egui::pos2(mid1, y - h * 0.35),
                    egui::Align2::CENTER_BOTTOM,
                    format!("Q: {} pkts", fmt_q(q12)),
                    egui::FontId::proportional(11.0),
                    egui::Color32::LIGHT_YELLOW,
                );
                painter.arrow(
                    egui::pos2(b2.right() + 4.0, y),
                    egui::vec2((b3.left() - 4.0) - (b2.right() + 4.0), 0.0),
                    egui::Stroke::new(2.0, ac),
                );
                painter.text(
                    egui::pos2(mid2, y - h * 0.35),
                    egui::Align2::CENTER_BOTTOM,
                    format!("Q: {} pkts", fmt_q(q23)),
                    egui::FontId::proportional(11.0),
                    egui::Color32::LIGHT_YELLOW,
                );
                painter.text(
                    egui::pos2(b1.left() - 24.0, y),
                    egui::Align2::RIGHT_CENTER,
                    "In",
                    egui::FontId::proportional(12.0),
                    egui::Color32::GRAY,
                );
                painter.text(
                    egui::pos2(b3.right() + 24.0, y),
                    egui::Align2::LEFT_CENTER,
                    "Out",
                    egui::FontId::proportional(12.0),
                    egui::Color32::GRAY,
                );

                ui.add_space(8.0);
                ui.label("Latence (avec files) vs taille de paquet — axe Y en échelle log");

                let y_min = 1.0_f64;
                let y_max_display = (self.latency_budget_us * 5.0).max(10_000.0) as f64;
                let mut latency_vec = Vec::new();
                let mut budget_vec = Vec::new();
                for size in (64..=1500).step_by(32) {
                    let size_f = size as f32;
                    let inter = self.inter_arrival_us_for_size(size_f);
                    let total_with_queue =
                        self.total_latency_with_queuing_us(size_f, inter);
                    let lat = if total_with_queue.is_finite() {
                        total_with_queue as f64
                    } else {
                        y_max_display
                    };
                    let y = (lat).max(y_min).ln();
                    latency_vec.push([size_f as f64, y]);
                    budget_vec.push([
                        size_f as f64,
                        (self.latency_budget_us as f64).max(y_min).ln(),
                    ]);
                }
                let latency_curve =
                    Line::new(PlotPoints::from(latency_vec)).name("Latence avec files (µs)");
                let budget_curve =
                    Line::new(PlotPoints::from(budget_vec)).name("Budget (µs)");
                let selected_x = self.packet_size_bytes.max(1.0) as f64;
                Plot::new("latency_vs_packet_size")
                    .height(900.0)
                    .legend(egui_plot::Legend::default())
                    .allow_zoom(true)
                    .allow_drag(true)
                    .allow_scroll(true)
                    .allow_boxed_zoom(true)
                    .include_x(64.0)
                    .include_x(1500.0)
                    .y_axis_formatter(|mark, _range| {
                        let v = mark.value.exp();
                        if v >= 1000.0 {
                            format!("{:.0}", v)
                        } else if v >= 10.0 {
                            format!("{:.0}", v)
                        } else if v >= 1.0 {
                            format!("{:.1}", v)
                        } else {
                            format!("{:.2}", v)
                        }
                    })
                    .coordinates_formatter(
                        Corner::RightBottom,
                        CoordinatesFormatter::new(|pt, _bounds| {
                            let size_bytes = pt.x;
                            let inter = self.inter_arrival_us_for_size(size_bytes as f32);
                            let lat = self.total_latency_with_queuing_us(
                                size_bytes as f32,
                                inter,
                            );
                            let lat_str = if lat.is_finite() {
                                format!("{:.1}", lat)
                            } else {
                                "∞".to_string()
                            };
                            format!(
                                "taille ≈ {:.0} octets\nlatence (avec files) ≈ {} µs",
                                size_bytes, lat_str
                            )
                        }),
                    )
                    .show(ui, |plot_ui: &mut egui_plot::PlotUi| {
                        plot_ui.line(latency_curve);
                        plot_ui.line(budget_curve);
                        plot_ui.vline(
                            VLine::new(selected_x)
                                .color(egui::Color32::YELLOW)
                                .width(2.0)
                                .name("Taille sélectionnée"),
                        );
                    });
            });
        });
    }
}
