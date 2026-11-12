// SPDX-License-Identifier: LGPL-2.1-or-later
//! Name tables: EDID PNP manufacturer ids and CEC (IEEE OUI) vendor ids, for audio and display
//! makers. A missing entry means "not in the table", never a guess.

/// Manufacturer name for a three-letter PNP id.
pub fn pnp_manufacturer(id: &str) -> Option<&'static str> {
    Some(match id {
        "ACR" => "Acer",
        "AUS" => "ASUS",
        "BNQ" => "BenQ",
        "DEL" => "Dell",
        "DON" => "Denon",
        "GSM" => "LG Electronics",
        "HEC" => "Hisense",
        "HWP" => "HP",
        "LEN" => "Lenovo",
        "MAX" => "Maxdata",
        "MEI" => "Panasonic",
        "NEC" => "NEC",
        "ONK" => "Onkyo",
        "PHL" => "Philips",
        "PIO" => "Pioneer",
        "SAM" => "Samsung",
        "SHP" => "Sharp",
        "SNY" => "Sony",
        "TOS" => "Toshiba",
        "VIZ" => "Vizio",
        "YMH" => "Yamaha",
        _ => return None,
    })
}

/// Vendor name for a CEC vendor id (IEEE OUI).
pub fn cec_vendor(oui: u32) -> Option<&'static str> {
    Some(match oui {
        0x0005cd => "Denon",
        0x0000f0 => "Samsung",
        0x00e091 => "LG",
        0x080046 => "Sony",
        0x008045 => "Panasonic",
        0x00903e => "Philips",
        0x000039 => "Toshiba",
        0x0009b0 => "Onkyo",
        0x00a0de => "Yamaha",
        0x00e036 => "Pioneer",
        0x08001f => "Sharp",
        0x001582 => "Pulse-Eight",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        assert_eq!(pnp_manufacturer("HEC"), Some("Hisense"));
        assert_eq!(pnp_manufacturer("ZZZ"), None);
        assert_eq!(cec_vendor(0x0005cd), Some("Denon"));
        assert_eq!(cec_vendor(1), None);
    }
}
