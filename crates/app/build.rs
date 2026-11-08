// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
fn main() {
    // Иконка .exe для Windows (только при сборке под Windows на Windows).
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            res.set("ProductName", "SkyUrok");
            res.set("FileDescription", "SkyUrok — расписание и замены");
            let _ = res.compile();
        }
    }
}
