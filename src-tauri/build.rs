fn main() {
    // Une seule commande pour le tableau de bord (« batlay »), autorisée
    // seulement aux fenêtres principale et Live (capabilities/main.json).
    // Déclarée ici, elle n'est plus accessible par défaut à toute page locale.
    let attributes = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&["batlay"]));
    tauri_build::try_build(attributes).expect("préparation de l'application Tauri impossible");
}
