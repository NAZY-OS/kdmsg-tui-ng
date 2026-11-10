#!/usr/bin/env bash
set -e  # Beendet bei Fehlern sofort

run_cargo_build() {
    local build_type="$1"
    local log_file="$2"
    local output_file
    local error_file
    local build_status
    local answer

    output_file="$(mktemp)"
    error_file="$(mktemp)"

    echo
    echo "Running cargo build $build_type..."

    if [[ "$build_type" == "--release" ]]; then
        cargo build --release >"$output_file" 2>"$error_file"
    else
        cargo build >"$output_file" 2>"$error_file"
    fi

    build_status=$?

    echo
    cat "$output_file"
    cat "$error_file"

    if [[ "$build_status" -ne 0 ]]; then
        cat "$output_file" "$error_file" > "$log_file"
        echo
        echo "Build failed with exit code $build_status."
        echo "Build output was saved to:"
        echo "./$log_file"
        rm -f "$output_file" "$error_file"
        return "$build_status"
    fi

    if grep -Eiq 'warning|warn:|error|error:|failed|failure|deprecated|unused|invalid|missing|unresolved|denied' "$error_file"; then
        echo
        echo "Warnings or other relevant messages were detected."
        read -r -p "Save this output to $log_file? Type yes/no (Enter = yes): " answer
        case "$answer" in
            ""|y|Y|yes|YES)
                cat "$output_file" "$error_file" > "$log_file"
                echo
                echo "Build output was saved to:"
                echo "./$log_file"
                ;;
            n|N|no|NO)
                echo "Build output was not saved."
                ;;
            *)
                echo "Invalid answer. Build output was not saved."
                ;;
        esac
    fi

    rm -f "$output_file" "$error_file"
    return 0
}

main() {
    local program_name
    local answer

    if [[ ! -f "Cargo.toml" ]]; then
        echo "Error: Cargo.toml not found!"
        return 1
    fi

    # Paketnamen extrahieren
    program_name=$(awk '
        /^\[package\]/ { in_package=1; next }
        /^\[/ && in_package { exit }
        in_package && /^[[:space:]]*name[[:space:]]*=/ {
            sub(/^[^"]*"/, "")
            sub(/".*$/, "")
            print
            exit
        }
    ' Cargo.toml)

    if [[ -z "$program_name" ]]; then
        echo "Error: Could not determine package name!"
        return 1
    fi

    echo "Package name: $program_name"
    echo

    # Cargo Update
    read -p "Run cargo update? [y/N] " answer
    case "$answer" in
        [yY]|[yY][eE][sS])
            echo "Running cargo update..."
            cargo update || { echo "Error: cargo update failed"; return 1; }
            ;;
        *)
            echo "Skipping cargo update."
            ;;
    esac

    # Test Build
    if ! run_cargo_build "" "compile-error.out.log"; then
        return 1
    fi
    echo "Test build completed successfully."

    # Release Build
    if ! run_cargo_build "--release" "to-fix.out.log"; then
        return 1
    fi

    # Binary prüfen
    if [[ ! -f "target/release/$program_name" ]]; then
        echo "Error: Binary not found at target/release/$program_name"
        return 1
    fi

    chmod +x "target/release/$program_name"

    # INSTALLATION NACH /sbin (GARANTIERT)
    echo
    echo "=== INSTALLING TO /sbin ==="
    echo "Copying binary to /sbin/$program_name..."
    sudo cp "target/release/$program_name" "/sbin/$program_name"
    sudo chmod 755 "/sbin/$program_name"

    # Überprüfen ob Installation erfolgreich war
    if [[ ! -f "/sbin/$program_name" ]]; then
        echo "ERROR: Installation to /sbin failed!"
        return 1
    fi

    echo "✅ Successfully installed to /sbin/$program_name"
    echo "Binary size: $(du -h "/sbin/$program_name" | cut -f1)"
    echo

    # Service-Datei installieren
    echo "Installing systemd service..."
    sudo cp "service/kdmsg-tui.service" "/lib/systemd/system/"
    sudo chmod 644 "/lib/systemd/system/kdmsg-tui.service"

    # systemd neu laden
    echo "Reloading systemd..."
    sudo systemctl daemon-reload
    sudo systemctl enable --now kdmsg-tui.service

    # Benutzerabfrage
    read -p "Run the program? [Y/n] " answer
    case "$answer" in
        ""|[yY]|[yY][eE][sS])
            echo "Starting program..."
            sudo "/sbin/$program_name"
            ;;
        *)
            echo "Program not started."
            ;;
    esac
}

main "$@"
