//! `retrox ship`: stock ships, their fits and what they add up to.

use std::process::ExitCode;

use retrox_content::ships::{HULLS, stock};
use retrox_sim::ship::{Load, Loadout, Mount, ShipStats};

pub fn print(hull: Option<&str>) -> ExitCode {
    let Some(hull) = hull else {
        print_table();
        return ExitCode::SUCCESS;
    };
    match stock(hull) {
        Some(fit) => {
            print_fit(&fit);
            ExitCode::SUCCESS
        }
        None => {
            let known: Vec<&str> = HULLS.iter().map(|h| h.id).collect();
            eprintln!("unknown hull {hull:?}; known: {}", known.join(", "));
            ExitCode::FAILURE
        }
    }
}

fn print_table() {
    println!(
        "{:<18} {:<7} {:<7} {:>7} {:>6} {:>5} {:>6} {:>6} {:>7} {:>7} {:>6} {:>9}",
        "hull",
        "size",
        "role",
        "mass t",
        "len m",
        "g",
        "g lad",
        "180° s",
        "dps",
        "shield",
        "cargo",
        "combat"
    );
    for hull in HULLS {
        let s = stock(hull.id).expect("every hull has a stock fit").stats();
        let laden = s.laden();
        let combat = match s.time_to_overheat(s.combat) {
            Some(secs) => format!("{secs:.0} s"),
            None => "∞".to_owned(),
        };
        println!(
            "{:<18} {:<7} {:<7} {:>7.0} {:>6.0} {:>5.1} {:>6.1} {:>6.1} {:>7.0} {:>7.0} {:>6.0} {:>9}",
            hull.id,
            format!("{:?}", hull.size),
            format!("{:?}", hull.role),
            s.mass_t,
            s.length_m,
            s.acceleration() / 9.81,
            laden.acceleration() / 9.81,
            s.time_to_turn(180.0).unwrap_or(f64::INFINITY),
            s.dps,
            s.shield,
            s.cargo_t,
            combat,
        );
    }
}

fn print_fit(fit: &Loadout) {
    let hull = fit.hull();
    println!("{} ({:?} {:?})", hull.name, hull.size, hull.role);
    for (mount, section) in fit.mounts() {
        let Some(section) = section else {
            println!("  (empty mount)");
            continue;
        };
        let fixed = matches!(mount, Mount::Fixed(_));
        println!(
            "  {}{}",
            section.section.name,
            if fixed { "" } else { " [swappable]" }
        );
        for (slot, module) in section.slots() {
            let module = module.map_or("(empty)", |m| m.name);
            println!(
                "    {:<8} {:<6} {module}",
                format!("{:?}", slot.kind),
                format!("{:?}", slot.size)
            );
        }
    }

    let s = fit.stats();
    let laden = s.laden();
    println!();
    println!(
        "  mass        {:.1} t empty, {:.1} t laden",
        s.mass_t, laden.mass_t
    );
    println!("  length      {:.0} m", s.length_m);
    println!(
        "  thrust      {:.0} kN  ({:.1} g empty, {:.1} g laden)",
        s.thrust_kn,
        s.acceleration() / 9.81,
        laden.acceleration() / 9.81
    );
    match (s.time_to_turn(180.0), laden.time_to_turn(180.0)) {
        (Some(empty), Some(full)) => println!(
            "  turning     {:.0} kN·m RCS, 180° in {empty:.2} s empty, {full:.2} s laden",
            s.rcs_torque_knm
        ),
        _ => println!("  turning     no RCS"),
    }
    println!("  structure   {:.0}", s.structure);
    println!(
        "  shield      {:.0}  (+{:.0}/s)",
        s.shield, s.shield_regen_per_s
    );
    println!(
        "  weapons     {:.0} dps, {:.1} km",
        s.dps, s.weapon_range_km
    );
    println!("  radar       {:.0} km", s.radar_range_km);
    println!("  cargo       {:.0} t", s.cargo_t);
    println!("  reactor     {:.1} MW", s.power_supply_mw);
    println!(
        "  radiators   {:.1} MW shed, {:.0} MJ capacity",
        s.heat_dissipation_mw, s.heat_capacity_mj
    );
    println!();
    print_load("cruise", &s, s.cruise);
    print_load("combat", &s, s.combat);
}

fn print_load(name: &str, s: &ShipStats, load: Load) {
    let heat = match s.time_to_overheat(load) {
        Some(secs) => format!("overheats in {secs:.0} s"),
        None => "sustainable".to_owned(),
    };
    println!(
        "  {name:<8} power {:>6.1}/{:.1} MW ({:+.1})  heat {:>6.1} MW, {heat}",
        load.power_mw,
        s.power_supply_mw,
        s.power_margin(load),
        load.heat_mw,
    );
}
