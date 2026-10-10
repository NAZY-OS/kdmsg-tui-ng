#!/usr/bin/env bash

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

    # Always create a log when the build fails.
    if [[ "$build_status" -ne 0 ]]; then
        cat "$output_file" "$error_file" > "$log_file"

        echo
        echo "Build failed with exit code $build_status."
        echo "Build output was saved to:"
        echo "./$log_file"

        rm -f "$output_file" "$error_file"
        return "$build_status"
    fi

    # Check stderr for warnings, errors, and other relevant messages.
    if grep -Eiq \
        'warning|warn:|error|error:|failed|failure|deprecated|unused|invalid|missing|unresolved|denied' \
        "$error_file"; then

        echo
        echo "Warnings or other relevant messages were detected."

        read -r -p \
            "Save this output to $log_file? Type yes/no (Enter = yes): " \
            answer

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
        echo "Error: Cargo.toml was not found."
        return 1
    fi

    # Determine the package name from Cargo.toml.
    program_name="$(
        awk '
            /^\[package\]/ {
                in_package = 1
                next
            }

            /^\[/ && in_package {
                exit
            }

            in_package && /^[[:space:]]*name[[:space:]]*=/ {
                sub(/^[^"]*"/, "")
                sub(/".*$/, "")
                print
                exit
            }
        ' Cargo.toml
    )"

    if [[ -z "$program_name" ]]; then
        echo "Error: Could not determine the package name from Cargo.toml."
        return 1
    fi

    echo "Package name: $program_name"
    echo
    cat Cargo.toml
    echo

    read -r -p \
        "Run cargo update? Type yes/no (Enter = no): " \
        answer

    case "$answer" in
        y|Y|yes|YES)
            echo
            echo "Running cargo update..."

            if ! cargo update; then
                echo "Error: cargo update failed."
                return 1
            fi
            ;;
        ""|n|N|no|NO)
            echo "Skipping cargo update."
            ;;
        *)
            echo "Invalid answer. Skipping cargo update."
            ;;
    esac

    # Regular test build.
    if ! run_cargo_build "" "compile-error.out.log"; then
        return 1
    fi

    echo
    echo "Test build completed successfully."

    # Release build.
    if ! run_cargo_build "--release" "to-fix.out.log"; then
        return 1
    fi

    if [[ ! -f "target/release/$program_name" ]]; then
        echo
        echo "Error: Release binary was not found:"
        echo "./target/release/$program_name"
        return 1
    fi

    chmod +x "target/release/$program_name"

    echo
    echo "Program is available at:"
    echo "./target/release/$program_name"

    echo
    read -r -p \
        "Run the program? Type yes/no (Enter = yes): " \
        answer

    case "$answer" in
        ""|y|Y|yes|YES)
            echo
            echo "Starting program..."
            "./target/release/$program_name"
            ;;
        n|N|no|NO)
            echo "Program was not started."
            ;;
        *)
            echo "Invalid answer. Program was not started."
            ;;
    esac
}

main "$@"
