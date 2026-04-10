// #region Imports
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::json;
// #endregion

// #region Functions

/// Generate an ipynb JSON structure for a new Fabric notebook.
///
/// Inputs:
///   - kernel: "python" or "pyspark"
///   - lakehouse_id: optional lakehouse GUID for default lakehouse
///   - lakehouse_name: optional lakehouse display name
///   - warehouse_id: optional warehouse GUID
///   - warehouse_name: optional warehouse display name
///   - workspace_id: workspace GUID (needed for trident references)
///
/// Returns: (ipynb_json_value, base64_encoded_string)
pub fn generate_ipynb(
    kernel: &str,
    lakehouse_id: Option<&str>,
    lakehouse_name: Option<&str>,
    warehouse_id: Option<&str>,
    warehouse_name: Option<&str>,
    workspace_id: &str,
) -> (serde_json::Value, String) {
    let (kernel_name, language_group, kernelspec_name, language) = match kernel {
        "pyspark" => (
            "synapse_pyspark",
            "synapse_pyspark",
            "python3",
            "python",
        ),
        _ => ("jupyter", "jupyter_python", "jupyter", "python"),
    };

    let kernelspec_display = if kernel == "python" { "Jupyter" } else { "Python 3" };

    let jupyter_kernel_name = if kernel == "python" {
        "python3.11"
    } else {
        "synapse_pyspark"
    };

    let mut metadata = json!({
        "kernel_info": {
            "name": kernel_name
        },
        "kernelspec": {
            "name": kernelspec_name,
            "display_name": kernelspec_display,
            "language": language
        },
        "language_info": {
            "name": language,
            "version": ""
        },
        "microsoft": {
            "language": language,
            "language_group": language_group,
            "ms_spell_check": {
                "ms_spell_check_language": "en"
            }
        },
        "nteract": {
            "version": "nteract-front-end@1.0.0"
        },
        "widgets": {},
        "spark_compute": {
            "compute_id": "/trident/default"
        }
    });

    // Add jupyter kernel_name for python kernels
    if kernel == "python" {
        metadata["kernel_info"]["jupyter_kernel_name"] = json!(jupyter_kernel_name);
    }

    // Build dependencies
    let mut dependencies = json!({});

    if let (Some(lh_id), Some(lh_name)) = (lakehouse_id, lakehouse_name) {
        dependencies["lakehouse"] = json!({
            "default_lakehouse": lh_id,
            "default_lakehouse_name": lh_name,
            "default_lakehouse_workspace_id": workspace_id,
            "known_lakehouses": [
                {
                    "id": lh_id
                }
            ]
        });
    }

    if let (Some(wh_id), Some(wh_name)) = (warehouse_id, warehouse_name) {
        dependencies["warehouse"] = json!({
            "default_warehouse": wh_id,
            "default_warehouse_name": wh_name,
            "default_warehouse_workspace_id": workspace_id,
            "known_warehouses": [
                {
                    "id": wh_id,
                    "type": "Datawarehouse"
                }
            ]
        });
    }

    if dependencies.as_object().map_or(false, |o| !o.is_empty()) {
        metadata["dependencies"] = dependencies;
    }

    let ipynb = json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": metadata,
        "cells": [
            {
                "cell_type": "code",
                "metadata": {
                    "microsoft": {
                        "language": language,
                        "language_group": language_group
                    }
                },
                "source": [""],
                "outputs": [],
                "execution_count": null
            }
        ]
    });

    let ipynb_str = serde_json::to_string_pretty(&ipynb).unwrap();
    let encoded = BASE64.encode(ipynb_str.as_bytes());

    (ipynb, encoded)
}

// #endregion
