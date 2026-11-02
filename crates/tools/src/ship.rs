//! `retrox ship`: a stock ship's fit and what it adds up to.

use std::process::ExitCode;

use retrox_content::ships::{HULLS, stock_interceptor};
use retrox_sim::ship::{Load, Loadout, ShipStats};

pub fn print(hull: &str) -> ExitCode {
    let fit = match hull {
        "interceptor" => stock_interceptor(),
        _ => {
            let known: Vec<&str> = HULLS.iter().map(|h| h.id).collect();
            eprintln!("unknown hull {hull:?}; known: {}", known.join(", "));
            return ExitCode::FAILURE;
        }
    };
    print_fit(&fit);
    ExitCode::SUCCESS
}

fn print_fit(fit: &Loadout) {
    let hull = fit.hull();
    println!("{} ({:?} hull, {} t)", hull.name, hull.size, hull.mass_t);
    for (slot, module) in fit.slots() {
        let module = module.map_or("(empty)", |m| m.name);
        println!(
            "  {:<8} {:<6} {module}",
            format!("{:?}", slot.kind),
            format!("{:?}", slot.size)
        );
    }

    let s = fit.stats();
    println!();
    println!("  mass        {:.1} t", s.mass_t);
    println!(
        "  thrust      {:.0} kN  ({:.1} m/s², {:.1} g)",
        s.thrust_kn,
        s.acceleration(),
        s.acceleration() / 9.81
    );
    if let Some(flip) = s.time_to_turn(180.0) {
        println!(
            "  turning     {:.0} kN·m RCS, 180° in {flip:.2} s",
            s.rcs_torque_knm
        );
    } else {
        println!("  turning     no RCS");
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
        "  {name:<8} power {:>5.1}/{:.1} MW ({:+.1})  heat {:>5.1} MW, {heat}",
        load.power_mw,
        s.power_supply_mw,
        s.power_margin(load),
        load.heat_mw,
    );
}
