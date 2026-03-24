#!/bin/bash
# N'ajoutez JAMAIS ce fichier au contrôle de version git.
# Utilisez ce script pour démarrer le serveur avec votre token privé.
export TELEGRAM_TOKEN="8797117531:AAHjBPTLNtwvk2KBd2d5dy8w6_mCGyR7LrI"

# Lancement du serveur...
cargo run --color=always -- --config config.toml
