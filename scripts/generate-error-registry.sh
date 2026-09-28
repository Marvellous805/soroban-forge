#!/usr/bin/env bash
# Generate machine-readable ForgeError registry from Rust source
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Parse the ForgeError enum from Rust source
ERROR_FILE="crates/shared-utils/src/errors.rs"
JSON_OUTPUT="errors.json"
TS_OUTPUT="packages/typescript-sdk/src/errors.generated.ts"

if [ ! -f "$ERROR_FILE" ]; then
    echo "Error: $ERROR_FILE not found" >&2
    exit 1
fi

# Extract enum variants and documentation
parse_errors() {
    local in_enum=false
    local current_doc=""
    local errors=""
    
    while IFS= read -r line; do
        # Start of ForgeError enum
        if [[ $line =~ ^pub\ enum\ ForgeError ]]; then
            in_enum=true
            continue
        fi
        
        # End of enum
        if [[ $in_enum == true && $line =~ ^\} ]]; then
            break
        fi
        
        if [[ $in_enum == true ]]; then
            # Documentation line
            if [[ $line =~ ^[[:space:]]*///[[:space:]]*(.*) ]]; then
                if [[ -n $current_doc ]]; then
                    current_doc="$current_doc ${BASH_REMATCH[1]}"
                else
                    current_doc="${BASH_REMATCH[1]}"
                fi
            # Enum variant
            elif [[ $line =~ ^[[:space:]]*([A-Za-z][A-Za-z0-9]*)[[:space:]]*=[[:space:]]*([0-9]+), ]]; then
                local name="${BASH_REMATCH[1]}"
                local code="${BASH_REMATCH[2]}"
                if [[ -n $errors ]]; then
                    errors="$errors,"
                fi
                errors="$errors"$'\n'"    { \"code\": $code, \"name\": \"$name\", \"doc\": \"$current_doc\" }"
                current_doc=""
            fi
        fi
    done < "$ERROR_FILE"
    
    echo "$errors"
}

# Generate JSON
generate_json() {
    local errors="$(parse_errors)"
    cat > "$JSON_OUTPUT" << EOF
{
  "schema": 1,
  "errors": [$errors
  ]
}
EOF
}

# Generate TypeScript
generate_typescript() {
    local errors="$(parse_errors)"
    
    # Convert JSON errors to TypeScript
    local ts_errors=""
    while IFS= read -r line; do
        if [[ $line =~ \"code\":[[:space:]]*([0-9]+).*\"name\":[[:space:]]*\"([^\"]+)\" ]]; then
            local code="${BASH_REMATCH[1]}"
            local name="${BASH_REMATCH[2]}"
            if [[ -n $ts_errors ]]; then
                ts_errors="$ts_errors,"
            fi
            ts_errors="$ts_errors"$'\n'"  [$code]: \"$name\""
        fi
    done <<< "$errors"
    
    cat > "$TS_OUTPUT" << EOF
// Generated file. Do not edit manually.
// Generated from crates/shared-utils/src/errors.rs

/**
 * Mapping of ForgeError codes to their names.
 */
export const FORGE_ERRORS: Record<number, string> = {$ts_errors
};

/**
 * Get the name of a ForgeError by its numeric code.
 * @param code The error code
 * @returns The error name, or undefined if not found
 */
export function forgeErrorName(code: number): string | undefined {
  return FORGE_ERRORS[code];
}
EOF
}

case "${1:-}" in
    json)
        generate_json
        echo "Generated $JSON_OUTPUT"
        ;;
    typescript)
        generate_typescript
        echo "Generated $TS_OUTPUT"
        ;;
    *)
        generate_json
        generate_typescript
        echo "Generated $JSON_OUTPUT and $TS_OUTPUT"
        ;;
esac