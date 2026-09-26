//! Catálogo espectral da luz — 16 bandas, 400→700nm.
//!
//! Cada horário do dia tem um SPD (*spectral power distribution*) modelado
//! com física simplificada: sol = corpo negro (Planck) cuja temperatura cai
//! com a elevação solar (âmbar no horizonte), céu = espalhamento Rayleigh
//! (λ⁻⁴, dominante no crepúsculo), noite = luar (sol ~4100K refletido na
//! superfície cinza da Lua). Itens ganham espectros canônicos: fogo (quente)
//! e neon frio.
//!
//! O SPD é convertido para **RGB linear** via as funções de observador CIE
//! 1931 (2°) amostradas nas mesmas 16 bandas — o "3 números" que o pipeline
//! RGB (pass de luz) consome. Um CCT único comprimiria o espectro num ponto
//! na linha de Planck e perderia tint (Duv), picos estreitos e o céu
//! não-Planckiano; integrar o SPD pelas CMFs é a projeção perceptualmente
//! exata para um render tricromático.

pub const BAND_COUNT: usize = 16;
/// Comprimentos de onda (nm) das bandas do catálogo, 400→700 a cada 20nm.
pub const BANDS_NM: [f32; BAND_COUNT] =
    [400.0, 420.0, 440.0, 460.0, 480.0, 500.0, 520.0, 540.0, 560.0, 580.0, 600.0, 620.0, 640.0, 660.0, 680.0, 700.0];
/// Funções de observador CIE 1931 (2°), alinhadas a `BANDS_NM`.
pub const CMF_1931_2DEG: [[f32; 3]; BAND_COUNT] = [
    [0.014310, 0.000396, 0.067850],
    [0.134380, 0.004000, 0.645600],
    [0.34828, 0.023, 1.74706],
    [0.2908, 0.06, 1.6692],
    [0.095640, 0.139020, 0.812950],
    [0.004900, 0.323000, 0.272000],
    [0.063270, 0.710000, 0.078250],
    [0.290400, 0.954000, 0.020300],
    [0.594500, 0.995000, 0.003900],
    [0.916300, 0.870000, 0.001650],
    [1.0622, 0.631, 0.0008],
    [0.854450, 0.381000, 0.000190],
    [0.447900, 0.175000, 0.000020],
    [0.164900, 0.061000, 0.000000],
    [0.046770, 0.017000, 0.000000],
    [0.011359, 0.004102, 0.000000],
];
/// Matriz linear sRGB (D65).
const SRGB_FROM_XYZ: [[f32; 3]; 3] = [
    [3.2406, -1.5372, -0.4986],
    [-0.9689, 1.8758, 0.0415],
    [0.0557, -0.2040, 1.0570],
];

/// SPD de 16 bandas. Escala absoluta é arbitrária até a normalização pelo
/// meio-dia em `to_xyz_d65`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Spd(pub [f32; BAND_COUNT]);

impl Spd {
    pub const fn new(bands: [f32; BAND_COUNT]) -> Self {
        Self(bands)
    }

    /// Integra SPD × CMF (trapézio, Δλ = 20nm) → CIE XYZ, na escala em que o
    /// MEIO-DIA (12:00) tem Y = 1 (branco de referência).
    pub fn to_xyz_d65(&self) -> [f32; 3] {
        let y_noon = xyz_raw(&daylight_spd(12.0))[1];
        let k = 1.0 / y_noon.max(1e-9);
        let xyz = xyz_raw(self);
        [xyz[0] * k, xyz[1] * k, xyz[2] * k]
    }

    /// RGB linear (sRGB D65) após integração pelas CMFs.
    pub fn to_linear_rgb(&self) -> [f32; 3] {
        let xyz = self.to_xyz_d65();
        let mut out = [0.0f32; 3];
        for (i, row) in SRGB_FROM_XYZ.iter().enumerate() {
            out[i] = row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2];
        }
        out.map(|c| c.max(0.0))
    }
}

fn xyz_raw(spd: &Spd) -> [f32; 3] {
    let mut xyz = [0.0f32; 3];
    for i in 0..BAND_COUNT - 1 {
        let w = (BANDS_NM[i + 1] - BANDS_NM[i]) * 0.5;
        for c in 0..3 {
            xyz[c] += (spd.0[i] * CMF_1931_2DEG[i][c] + spd.0[i + 1] * CMF_1931_2DEG[i + 1][c]) * w;
        }
    }
    xyz
}

pub fn smoothstep01(e0: f32, e1: f32, x: f32) -> f32 {
    if e0 >= e1 {
        return 0.0;
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Radiância espectral do corpo negro (Planck), escala relativa.
fn blackbody(nm: f32, temp_k: f32) -> f32 {
    const C2: f64 = 1.4388e-2; // m·K
    if temp_k <= 0.0 {
        return 0.0;
    }
    let wl = nm as f64 * 1e-9;
    let t = temp_k as f64;
    let e = (C2 / (wl * t)).min(700.0);
    (1.0 / (wl.powi(5) * (e.exp() - 1.0))) as f32
}

fn gauss(nm: f32, center: f32, width: f32) -> f32 {
    let d = nm - center;
    (-(d * d) / (2.0 * width * width)).exp()
}

/// Interpolação linear por trechos sobre a agenda `(hora, valor)` de marcos.
fn seg_lerp(h: f32, pts: &[(f32, f32)]) -> f32 {
    for w in pts.windows(2) {
        if h <= w[1].0 {
            let t = (h - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    pts[pts.len() - 1].1
}

/// Elevação solar (rad) — AGENDA EXPLÍCITA de marcos (nada de fórmula
/// "sorteada"): cada horário-chave tem ângulo fixo, interpolação linear
/// entre eles. Os efeitos de cor caem exatamente na hora certa.
///
/// | hora | el  | efeito                          |
/// |------|-----|---------------------------------|
/// | 00:00| −20°| meia-noite (luar, céu morto)    |
/// | 04:00| −6° | hora azul  (azul forte)         |
/// | 08:00| +5° | nascer     (âmbar dourado)      |
/// | 12:00|+65° | meio-dia   (branco pleno)       |
/// | 16:00| +5° | pôr-do-sol (âmbar dourado)      |
/// | 20:00|−10° | crepúsculo (luar + azul fraco)  |
/// | 24:00|−20° | meia-noite                      |
pub fn solar_elevation(hour: f32) -> f32 {
    let h = (hour % 24.0 + 24.0) % 24.0;
    const SCHEDULE: [(f32, f32); 7] = [
        (0.0, -20.0),
        (4.0, -6.0),
        (8.0, 5.0),
        (12.0, 65.0),
        (16.0, 5.0),
        (20.0, -10.0),
        (24.0, -20.0),
    ];
    seg_lerp(h, &SCHEDULE).to_radians()
}

/// Envelope de luminosidade global 0.10..1 — TRIÂNGULO linear no tempo:
/// 1.0 EXATO ao meio-dia (12h), piso 10% à meia-noite (0h/24h), subindo em
/// linha reta de 00:00 até 12:00 e descendo em linha reta até 24:00. Cada tick
/// do slider anda o mesmo degrau de claridade.
pub fn day_factor(hour: f32) -> f32 {
    let u = (hour % 24.0 + 24.0) % 24.0;
    0.10 + 0.90 * (1.0 - (u / 12.0 - 1.0).abs()).clamp(0.0, 1.0)
}

/// Temperatura de cor (K) do sol direto pela elevação: 2400K no horizonte
/// (âmbar do fim de tarde) → 6000K com o sol alto (branco dia). Rampa bem
/// LARGA (2°..40°): o âmbar esfria para branco por ~5h de manhã/tarde em
/// vez de virar num instante.
fn color_temp(el_deg: f32) -> f32 {
    let ramp = smoothstep01(2.0, 40.0, el_deg);
    2400.0 + (6000.0 - 2400.0) * ramp
}

/// SPD composto do ambiente (luz global) para a hora `h` (0..24).
///
/// Sol direto (Planck com temperatura pela elevação) + céu Rayleigh (azul,
/// forte no crepúsculo e leve ao meio-dia) + luar à noite. A escala relativa
/// de cada componente é calibrada para dar branco ao meio-dia e noite escura
/// e fria na madrugada.
fn daylight_spd(hour: f32) -> Spd {
    let el_deg = solar_elevation(hour).to_degrees();
    // Sol DIRETO é gateado por rampa LARGA de -8° a +25°: não ilumina bem
    // abaixo do horizonte, mas o brilho direto sobe gradualmente por várias
    // horas da manhã (e desce na tarde) — em vez de ligar/desligar de vez.
    let sun_on = smoothstep01(-8.0, 25.0, el_deg);
    let t = color_temp(el_deg);
    // "below": 1 quando o disco solar está baixo (hora azul) — rampa BEM
    // larga (-7°..-0.5°): a faixa azul dura ~1.5h em vez de alguns minutos.
    let below = 1.0 - smoothstep01(-7.0, -0.5, el_deg);
    // Céu Rayleigh: leve durante o dia (complemento frio), forte na hora azul,
    // some na noite profunda (fade até -12°).
    let fade_night = smoothstep01(-12.0, -2.0, el_deg);
    let sky_b = 0.08 * sun_on + 2.0 * below * fade_night;
    // Lua: some perto do horizonte (el > -3°) e plena na noite profunda.
    let moon_w = 1.0 - smoothstep01(-12.0, -3.0, el_deg);
    // Céu noturno frio: nasce na hora azul/crepúsculo e domina a NOITE PROFUNDA
    // (el < -12°), onde o fade_night zera o azul — dá o tom frio à meia-noite
    // sem deixá-la brilhante (o env de 5% segura a intensidade).
    let night_sky = (1.0 - smoothstep01(-12.0, -4.0, el_deg)).powi(2);
    let mut bands = [0.0f32; BAND_COUNT];
    for (i, &nm) in BANDS_NM.iter().enumerate() {
        let sun = blackbody(nm, t) / blackbody(500.0, t).max(1e-9);
        let sky = (400.0 / nm).powi(4); // Rayleigh ~ λ⁻⁴
        let moon = blackbody(nm, 4100.0) / blackbody(500.0, 4100.0).max(1e-9);
        bands[i] = sun * sun_on + sky * (sky_b + 0.35 * night_sky) + moon * 0.07 * moon_w;
    }
    Spd(bands)
}

/// Cor ambiente RGB linear (com intensidade) para a hora `h` (0..24): o
/// espectro (sol gateado + céu + lua) é dimensionado pelo envelope
/// `day_factor` — 1.0 pleno ao meio-dia, piso ~5% à meia-noite, marcos
/// explícitos com transição linear. Piso noturno sobre a LUMINÂNCIA (que é o
/// que o olho percebe como brilho) para as luzes de item continuarem visíveis
/// na madrugada sem nunca "clarear de novo" quando o matiz gira (azul do
/// crepúsculo → quente do luar).
pub fn ambient_rgb(hour: f32) -> [f32; 3] {
    let mut c = daylight_spd(hour).to_linear_rgb();
    c = c.map(|v| v * day_factor(hour));
    // Piso NOTURNO sobre a luminância — diferente do piso por canal máximo,
    // que fazia o crepúsculo parecer CLAREAR quando o azul (peso de luminância
    // baixo) dava lugar ao luar quente (peso alto). Com o piso na luminância a
    // noite desce suave e monotônica até a meia-noite.
    const NIGHT_FLOOR: f32 = 0.030;
    let lum = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    if lum < NIGHT_FLOOR {
        let k = NIGHT_FLOOR / lum.max(1e-6);
        c = c.map(|v| v * k);
    }
    c.map(|v| v.clamp(0.0, 1.0))
}

fn normalize_max(c: [f32; 3]) -> [f32; 3] {
    let m = c[0].max(c[1]).max(c[2]).max(1e-9);
    [c[0] / m, c[1] / m, c[2] / m]
}

/// Espectro de chama (fogo/vela): corpo negro ~1900K — contínuo, pico no
/// vermelho-laranja, sem nada no azul.
pub fn fire_spd() -> Spd {
    let mut b = [0.0f32; BAND_COUNT];
    for (i, &nm) in BANDS_NM.iter().enumerate() {
        b[i] = blackbody(nm, 1900.0) / blackbody(500.0, 1900.0).max(1e-9);
    }
    Spd(b)
}

/// Espectro de neon frio (luz própria fria de item): tubo com picos azul
/// ~470nm + ciano ~510nm + leve verde — cor fria de decoração.
pub fn neon_cold_spd() -> Spd {
    let mut b = [0.0f32; BAND_COUNT];
    for (i, &nm) in BANDS_NM.iter().enumerate() {
        b[i] = 1.00 * gauss(nm, 470.0, 22.0)
            + 0.70 * gauss(nm, 510.0, 26.0)
            + 0.30 * gauss(nm, 560.0, 30.0)
            + 0.15 * gauss(nm, 430.0, 18.0);
    }
    Spd(b)
}

/// RGB linear do fogo, com o canal mais forte em 1 (só o matiz importa).
pub fn fire_rgb() -> [f32; 3] {
    normalize_max(fire_spd().to_linear_rgb())
}

/// RGB linear do neon frio, com o canal mais forte em 1.
pub fn neon_cold_rgb() -> [f32; 3] {
    normalize_max(neon_cold_spd().to_linear_rgb())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bandas_sao_16_de_400_a_700() {
        assert_eq!(BAND_COUNT, 16);
        assert_eq!(BANDS_NM[0], 400.0);
        assert_eq!(BANDS_NM[15], 700.0);
    }

    #[test]
    fn meio_dia_da_branco() {
        let c = ambient_rgb(12.0);
        assert!(day_factor(12.0) > 0.999);
        for v in c {
            assert!((0.92..=1.05).contains(&v), "meio-dia deveria ser quase branco, got {c:?}");
        }
    }

    #[test]
    fn meia_noite_escura() {
        let noon = ambient_rgb(12.0);
        let night = ambient_rgb(0.0);
        assert_eq!(night, ambient_rgb(24.0));
        assert!(day_factor(0.0) < 0.11, "meia-noite no piso (10%), não breu");
        let n_max = night[0].max(night[1]).max(night[2]);
        let d_max = noon[0].max(noon[1]).max(noon[2]);
        assert!(d_max > 4.0 * n_max, "meia-noite deveria ser bem mais escura: {night:?} vs {noon:?}");
        assert!(n_max > 0.0, "meia-noite não pode ser preto absoluto");
        assert!(n_max >= d_max / 25.0, "piso precisa manter algo visível");
    }

    #[test]
    fn noite_nao_reclareia() {
        let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let a = lum(ambient_rgb(19.95)); // 19:57 · fim da hora azul
        let b = lum(ambient_rgb(20.16)); // 20:09 · sob o luar (era MAIS claro: bug relatado)
        let c2 = lum(ambient_rgb(22.00)); // noite profunda
        assert!(a + 1e-6 >= b, "crepúsculo reacendeu entre 19:57 e 20:09: {a} → {b}");
        assert!(b + 1e-6 >= c2, "noite voltou a clarear: {b} → {c2}");
    }

    #[test]
    fn por_do_sol_quente() {
        let c = ambient_rgb(16.0);
        assert!(c[0] > c[1] * 1.4 && c[0] > c[2] * 1.4, "pôr-do-sol quente (laranja), got {c:?}");
    }

    #[test]
    fn hora_azul_fria() {
        // Marco 04:00, el ≈ -6° (agenda explícita).
        let c = ambient_rgb(4.0);
        assert!(c[2] > c[0] * 1.15, "hora azul deveria puxar o azul, got {c:?}");
    }

    #[test]
    fn fogo_quente_e_neon_frio() {
        let f = fire_rgb();
        assert!(f[0] > f[1] && f[1] > f[2], "fogo vermelho > verde > azul, got {f:?}");
        let n = neon_cold_rgb();
        assert!(n[2] >= n[1] && n[1] > n[0], "neon frio azul/ciano dominante, got {n:?}");
    }

    #[test]
    fn elevacao_pico_ao_meio_dia() {
        let e = |h: f32| solar_elevation(h).to_degrees();
        // Agenda explícita: ângulos fixos nos marcos.
        assert!((e(0.0) + 20.0).abs() < 0.001);
        assert!((e(24.0) + 20.0).abs() < 0.001);
        assert!((e(4.0) + 6.0).abs() < 0.001, "hora azul = -6°");
        assert!((e(8.0) - 5.0).abs() < 0.001, "nascer = +5°");
        assert!((e(12.0) - 65.0).abs() < 0.001, "meio-dia = +65°");
        // Pico exatamente no meio-dia.
        assert!(e(11.9) < e(12.0) && e(12.1) < e(12.0));
        assert!(e(15.0) > e(16.0) && e(16.0) > e(17.0));
    }
}