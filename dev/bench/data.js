window.BENCHMARK_DATA = {
  "lastUpdate": 1791920378745,
  "repoUrl": "https://github.com/PHPantom-dev/phpantom_lsp",
  "entries": {
    "PHPantom Benchmarks": [
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2c119893ffe7b665ea2ffb83d791d489abce2200",
          "message": "Update performance tasks",
          "timestamp": "2026-10-10T21:08:27+02:00",
          "tree_id": "0dac723cbed644dabc1397672c6c763e250e868b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2c119893ffe7b665ea2ffb83d791d489abce2200"
        },
        "date": 1791659887811,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.439,
            "range": "± 0.274",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.046,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.115,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.144,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.237,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.295,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.181,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.271,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.142,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.057,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.053,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.066,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.469,
            "range": "± 0.039",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.154,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.334,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.078,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.119,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.099,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.071,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.233,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.157,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.908,
            "range": "± 0.09",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.168,
            "range": "± 0.052",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.144,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.45,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 21.146,
            "range": "± 0.291",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 1.207,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.085,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.079,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.33,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.341,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.448,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ba92a8ab47e7ea70032788c2fe7115e8fe077ed4",
          "message": "Faster analysis of closures in long files",
          "timestamp": "2026-10-10T21:43:23+02:00",
          "tree_id": "1deef55411e83d5b51a88b2631be7a81eef19779",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ba92a8ab47e7ea70032788c2fe7115e8fe077ed4"
        },
        "date": 1791661995250,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.079,
            "range": "± 0.156",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.04,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.089,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.131,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.224,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.229,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.999,
            "range": "± 0.045",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.982,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.098,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.051,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.045,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.049,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.931,
            "range": "± 0.058",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.188,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.305,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.053,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.045,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.12,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.056,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.169,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.99,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.834,
            "range": "± 0.105",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.003,
            "range": "± 0.067",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.921,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.358,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.69,
            "range": "± 0.15",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 1.229,
            "range": "± 0.055",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.059,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.056,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.24,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.198,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.296,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f50e3ce8432dd9246a835be43a4b381006c63e34",
          "message": "Improve performance of keyword parsing",
          "timestamp": "2026-10-10T21:44:50+02:00",
          "tree_id": "53601a58a1b24e89bbe97bb5022b51120e89152e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f50e3ce8432dd9246a835be43a4b381006c63e34"
        },
        "date": 1791662072082,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.49,
            "range": "± 0.155",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.113,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.15,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.233,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.275,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.163,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.374,
            "range": "± 0.073",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.136,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.058,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.058,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.066,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.262,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.143,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.334,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.068,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.122,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.081,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.072,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.235,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.167,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.114,
            "range": "± 0.165",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.171,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.022,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.39,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.852,
            "range": "± 0.241",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 1.167,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.092,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.088,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.294,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.34,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.45,
            "range": "± 0.008",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1c94a8a0cdc23c91941db6b591fb3291cecf55c3",
          "message": "Faster analysis of arrays filled under many `if`s",
          "timestamp": "2026-10-10T23:06:37+02:00",
          "tree_id": "4e796f57486e1f684e47492df83ff216fc6eb910",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1c94a8a0cdc23c91941db6b591fb3291cecf55c3"
        },
        "date": 1791666980891,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.519,
            "range": "± 0.249",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.113,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.156,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.236,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.294,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.171,
            "range": "± 0.04",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.292,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.139,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.06,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.057,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.069,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.199,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.143,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.341,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.079,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.123,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.093,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.072,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.231,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.146,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.806,
            "range": "± 0.056",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.154,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.011,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.385,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.8,
            "range": "± 0.086",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 1.148,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.08,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.079,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.302,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.335,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.446,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3f7b2e43b5dd6aacbb3eead1b8df748cf8637637",
          "message": "A value both branches could hold no longer says which branch ran",
          "timestamp": "2026-10-10T23:39:58+02:00",
          "tree_id": "087ce377f4fc04c5fb2efadf0a67db0586782350",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3f7b2e43b5dd6aacbb3eead1b8df748cf8637637"
        },
        "date": 1791668982349,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.622,
            "range": "± 0.073",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.044,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.1,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.148,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.246,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.263,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.145,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.303,
            "range": "± 0.063",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.106,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.056,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.051,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.055,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.714,
            "range": "± 0.056",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.191,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.352,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.057,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.051,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.136,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.059,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.054,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.191,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.128,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.672,
            "range": "± 0.038",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.143,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.928,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.349,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.681,
            "range": "± 0.19",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 1.38,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.069,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.065,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.267,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.225,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.333,
            "range": "± 0.007",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d5e315ce127d5fe8681c70906ca586b24636118c",
          "message": "Faster analysis of long method chains",
          "timestamp": "2026-10-10T23:47:42+02:00",
          "tree_id": "2284f1a4927fc5bc207d48add53a16cb9865387e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d5e315ce127d5fe8681c70906ca586b24636118c"
        },
        "date": 1791669444886,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.507,
            "range": "± 0.06",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.112,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.159,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.241,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.296,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.174,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.251,
            "range": "± 0.074",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.141,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.057,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.066,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.236,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.147,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.357,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.079,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.126,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.091,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.077,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.234,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.16,
            "range": "± 0.05",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.895,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.159,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.036,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.39,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.628,
            "range": "± 0.346",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.797,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.09,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.084,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.319,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.339,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.435,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "757c12f697f5c7e3bd87ff52cf6b9dd8e8d47e04",
          "message": "Faster analysis of arrays given a different shape under each of many\n`if`s",
          "timestamp": "2026-10-11T00:14:02+02:00",
          "tree_id": "aa16ca9b9693245efe63545536de1b97bc8d1265",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/757c12f697f5c7e3bd87ff52cf6b9dd8e8d47e04"
        },
        "date": 1791671033526,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.527,
            "range": "± 0.103",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.045,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.11,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.161,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.239,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.276,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.143,
            "range": "± 0.064",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.25,
            "range": "± 0.065",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.143,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.054,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.067,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.22,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.146,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.342,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.077,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.128,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.082,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.079,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.234,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.144,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.799,
            "range": "± 0.055",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.142,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.044,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.387,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.729,
            "range": "± 0.225",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.791,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.086,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.079,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.316,
            "range": "± 0.033",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.353,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.437,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7546a176ae4d1245edd5e3a9dd2e6de37188d39f",
          "message": "Faster loading of vendor and stub files",
          "timestamp": "2026-10-11T00:15:44+02:00",
          "tree_id": "1aff810cf9e79b0c3953e3ae30962df8bf6691e4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7546a176ae4d1245edd5e3a9dd2e6de37188d39f"
        },
        "date": 1791671118046,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.957,
            "range": "± 0.038",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.074,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.11,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.18,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.217,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.94,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.82,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.089,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.039,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.035,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.043,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.404,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.115,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.247,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.044,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.09,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.042,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.163,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.874,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.672,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.876,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.742,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.283,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 13.143,
            "range": "± 0.118",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.584,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.054,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.051,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.199,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.257,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.342,
            "range": "± 0.002",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ba312fe3800edab03f13d0356e9c2479409f234f",
          "message": "APCu functions return only the form the call selects",
          "timestamp": "2026-10-11T00:24:16+02:00",
          "tree_id": "e49c4c64a4619237f661d8ea6174c24ad97154f4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ba312fe3800edab03f13d0356e9c2479409f234f"
        },
        "date": 1791671643328,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.15,
            "range": "± 0.184",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.11,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.157,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.242,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.285,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.182,
            "range": "± 0.041",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.299,
            "range": "± 0.082",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.138,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.06,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.072,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.271,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.146,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.332,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.077,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.121,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.087,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.08,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.232,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.139,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.832,
            "range": "± 0.104",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.155,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.037,
            "range": "± 0.046",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.386,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.463,
            "range": "± 0.264",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.794,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.084,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.08,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.32,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.341,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.445,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6928d7b857db74250d48acf4eff8fdfa9161d5bf",
          "message": "Faster analysis of `switch` lookup tables",
          "timestamp": "2026-10-11T00:37:53+02:00",
          "tree_id": "632df371f773f8211baf223216f0c4d7c838b828",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6928d7b857db74250d48acf4eff8fdfa9161d5bf"
        },
        "date": 1791672431045,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.925,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.073,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.108,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.18,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.212,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.955,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.855,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.087,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.039,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.036,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.04,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.414,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.11,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.25,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.042,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.032,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.089,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.04,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.165,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.889,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.696,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.896,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.74,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.286,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 13.275,
            "range": "± 0.046",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.59,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.053,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.048,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.19,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.259,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.344,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c4d52f5026b8ab1ce6d782b618d4d2b9fafe0ed7",
          "message": "Organize imports on save",
          "timestamp": "2026-10-11T01:11:25+02:00",
          "tree_id": "b7bcf8ef2604292a1b828c7e8c80871a20ebd1f2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c4d52f5026b8ab1ce6d782b618d4d2b9fafe0ed7"
        },
        "date": 1791674388813,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.329,
            "range": "± 0.19",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.024,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.052,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.075,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.124,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.135,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.598,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.167,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.057,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.029,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.029,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.843,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.095,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.184,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.027,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.069,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.031,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.028,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.122,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.7,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 3.748,
            "range": "± 0.087",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.661,
            "range": "± 0.042",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.549,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.215,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 9.702,
            "range": "± 1.502",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.465,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.035,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.036,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.143,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.193,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.244,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fda7a2c909a55b01dc5c91f239e6fdf87b544325",
          "message": "Faster analysis of `if`/`elseif` lookup tables",
          "timestamp": "2026-10-11T02:23:43+02:00",
          "tree_id": "812aeda6bf072e0cb82c88c8ed470e1b986a6d14",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fda7a2c909a55b01dc5c91f239e6fdf87b544325"
        },
        "date": 1791678799728,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.99,
            "range": "± 0.151",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.113,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.155,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.235,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.296,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.167,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.402,
            "range": "± 0.09",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.14,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.063,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.07,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.346,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.147,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.36,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.05,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.128,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.094,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.076,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.23,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.147,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.871,
            "range": "± 0.125",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.142,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.035,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.385,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.246,
            "range": "± 0.155",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.783,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.095,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.083,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.343,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.336,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.441,
            "range": "± 0.011",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "caf392399ed288b1371f0ec0af6805492a67a66a",
          "message": "Update roadmap",
          "timestamp": "2026-10-11T02:29:33+02:00",
          "tree_id": "62d08792972aa971a644ebf306dad3147312f2e5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/caf392399ed288b1371f0ec0af6805492a67a66a"
        },
        "date": 1791679163725,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.009,
            "range": "± 0.078",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.115,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.157,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.235,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.288,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.17,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.415,
            "range": "± 0.066",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.14,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.061,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.072,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.327,
            "range": "± 0.051",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.151,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.358,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.079,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.055,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.122,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.09,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.07,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.233,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.166,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.12,
            "range": "± 0.265",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.158,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.054,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.388,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.69,
            "range": "± 0.253",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.786,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.091,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.08,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.325,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.345,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.448,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7e71a9ecb8062c8d8f4c5f75215aeb9559982292",
          "message": "`!== null` on a value that is one of several literals",
          "timestamp": "2026-10-11T04:35:03+02:00",
          "tree_id": "f769975248cef38db7d5c34996214871fd908ac0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7e71a9ecb8062c8d8f4c5f75215aeb9559982292"
        },
        "date": 1791686700955,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.013,
            "range": "± 0.168",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.108,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.163,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.236,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.281,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.169,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.242,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.143,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.061,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.051,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.073,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.294,
            "range": "± 0.033",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.154,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.336,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.08,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.052,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.127,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.09,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.081,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.232,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.152,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.906,
            "range": "± 0.132",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.157,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.049,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.392,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.621,
            "range": "± 0.285",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.793,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.088,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.076,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.324,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.337,
            "range": "± 0.029",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.437,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f622e524c693884524302d7a3867c6841606e9df",
          "message": "Fix test",
          "timestamp": "2026-10-11T04:37:26+02:00",
          "tree_id": "32f70555235db0d5747e200dd1a2fe87cdc4421c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f622e524c693884524302d7a3867c6841606e9df"
        },
        "date": 1791686828661,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.848,
            "range": "± 0.085",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.11,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.157,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.241,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.294,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.159,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.274,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.138,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.065,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.28,
            "range": "± 0.054",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.147,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.323,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.073,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.117,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.09,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.073,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.235,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.146,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.826,
            "range": "± 0.086",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.149,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.012,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.387,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.483,
            "range": "± 0.128",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.784,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.08,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.082,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.321,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.343,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.435,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "51848db138beb7a3680dd27a32f7404e33877623",
          "message": "`keyBy()` and `groupBy()` on an Eloquent collection keep the model type",
          "timestamp": "2026-10-11T04:56:46+02:00",
          "tree_id": "b16870bd9274ca3cd3685f49dc8680eeb6ffb893",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/51848db138beb7a3680dd27a32f7404e33877623"
        },
        "date": 1791687986617,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.002,
            "range": "± 0.464",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.049,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.114,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.151,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.245,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.295,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.169,
            "range": "± 0.042",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.265,
            "range": "± 0.069",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.141,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.062,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.068,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.169,
            "range": "± 0.063",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.157,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.341,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.074,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.121,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.094,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.069,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.235,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.175,
            "range": "± 0.054",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.238,
            "range": "± 0.263",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.169,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.034,
            "range": "± 0.041",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.395,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.645,
            "range": "± 1.007",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.811,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.093,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.081,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.299,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.334,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.447,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "28a4cd202c2928914704482adf97d0ca9925a01d",
          "message": "Faster diagnostics while editing",
          "timestamp": "2026-10-11T05:07:28+02:00",
          "tree_id": "14c09729cc409ac87baaa552159c4e9195c29726",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/28a4cd202c2928914704482adf97d0ca9925a01d"
        },
        "date": 1791688925422,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.959,
            "range": "± 0.068",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.045,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.114,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.145,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.246,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.302,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.158,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.451,
            "range": "± 0.055",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.146,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.064,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.072,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.278,
            "range": "± 0.04",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.15,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.341,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.078,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.047,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.13,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.08,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.076,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.232,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.149,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.872,
            "range": "± 0.053",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.169,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.028,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.392,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 16.993,
            "range": "± 0.232",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.795,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.084,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.074,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.33,
            "range": "± 0.035",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.34,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.454,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "05257829104d8d5cd1f8808246003d032f96fafa",
          "message": "Saving a method body refreshes the files whose types depend on it",
          "timestamp": "2026-10-11T05:31:50+02:00",
          "tree_id": "b5e17d1f4d29853ff6428413a03a60d4636dae5e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/05257829104d8d5cd1f8808246003d032f96fafa"
        },
        "date": 1791690089715,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.003,
            "range": "± 0.051",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.045,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.112,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.156,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.239,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.276,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.155,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.404,
            "range": "± 0.095",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.138,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.056,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.051,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.065,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.196,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.148,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.358,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.076,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.12,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.092,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.075,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.234,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.154,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.835,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.165,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.023,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.391,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.121,
            "range": "± 0.131",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.785,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.09,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.085,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.299,
            "range": "± 0.033",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.345,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.43,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d0d394f46328095db2dc0da3b0abbf8fe596e8ef",
          "message": "`$this` in a Pest test is the configured test case",
          "timestamp": "2026-10-11T05:53:42+02:00",
          "tree_id": "a27c6420487b3475a3e752a7bdb8b79f580fc7a3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d0d394f46328095db2dc0da3b0abbf8fe596e8ef"
        },
        "date": 1791691399249,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.991,
            "range": "± 0.072",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.046,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.116,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.145,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.242,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.295,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.17,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.255,
            "range": "± 0.081",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.143,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.053,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.066,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.251,
            "range": "± 0.039",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.148,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.36,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.069,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.052,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.126,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.093,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.07,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.235,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.144,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.857,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.164,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.008,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.388,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.026,
            "range": "± 0.061",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.798,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.081,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.08,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.321,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.34,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.444,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "549c114ca888d622d880802ddcff2921cf0cc36a",
          "message": "Improve Pest handeling",
          "timestamp": "2026-10-11T06:10:15+02:00",
          "tree_id": "c70d70e458dba3449c3a95bf0d6986c56191b545",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/549c114ca888d622d880802ddcff2921cf0cc36a"
        },
        "date": 1791692399229,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.998,
            "range": "± 0.307",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.045,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.114,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.156,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.242,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.291,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.17,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.246,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.141,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.056,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.07,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.271,
            "range": "± 0.144",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.156,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.344,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.126,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.082,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.076,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.239,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.142,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.779,
            "range": "± 0.21",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.147,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.032,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.392,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.118,
            "range": "± 0.575",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.792,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.088,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.088,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.33,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.349,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.435,
            "range": "± 0.012",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b502da6acca87c39e175af2d0b6f34ccfcb7c5a2",
          "message": "`@method` parameters defaulting to `null`",
          "timestamp": "2026-10-11T06:12:51+02:00",
          "tree_id": "b34bbfd69bda0ddf85bdf35c6de7b0fe50c21e52",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b502da6acca87c39e175af2d0b6f34ccfcb7c5a2"
        },
        "date": 1791692563097,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.019,
            "range": "± 0.127",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.048,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.111,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.159,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.248,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.299,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.234,
            "range": "± 0.087",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.296,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.154,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.066,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.057,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.07,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.299,
            "range": "± 0.082",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.16,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.362,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.069,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.133,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.091,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.078,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.237,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.167,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.121,
            "range": "± 0.173",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.184,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.042,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.403,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.645,
            "range": "± 0.528",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.801,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.092,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.087,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.316,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.341,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.448,
            "range": "± 0.025",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4a1b2a9b03f288fdcbf8aea3843a703f9e721399",
          "message": "`use X as Y` next to another import of the same short name",
          "timestamp": "2026-10-11T06:27:58+02:00",
          "tree_id": "42d49e72b25de69432bf0c5a9d6781cf3ebac40d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4a1b2a9b03f288fdcbf8aea3843a703f9e721399"
        },
        "date": 1791693473918,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.998,
            "range": "± 0.428",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.046,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.118,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.155,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.242,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.275,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.173,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.275,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.144,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.058,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.061,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.075,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.166,
            "range": "± 0.172",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.151,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.334,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.047,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.127,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.089,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.083,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.238,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.162,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.161,
            "range": "± 0.217",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.169,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.06,
            "range": "± 0.084",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.395,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.389,
            "range": "± 0.379",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.807,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.084,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.08,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.303,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.387,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.442,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6af15eb4272e1bf2fb1a5743024e54bffef90693",
          "message": "`object|null` return types",
          "timestamp": "2026-10-11T06:30:57+02:00",
          "tree_id": "a015bbb0dcd4a64668bc4263fdf52d8e10d25e72",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6af15eb4272e1bf2fb1a5743024e54bffef90693"
        },
        "date": 1791693646798,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.997,
            "range": "± 0.095",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.117,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.162,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.25,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.294,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.167,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.269,
            "range": "± 0.076",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.139,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.061,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.06,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.073,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.438,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.159,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.343,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.077,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.123,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.083,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.082,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.236,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.161,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.88,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.179,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.019,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.395,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.185,
            "range": "± 0.195",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.818,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.087,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.085,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.313,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.353,
            "range": "± 0.051",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.437,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "77c3618374a81a36ce90babbe37534ec05a10d4e",
          "message": "Lower memory use on large projects",
          "timestamp": "2026-10-11T06:34:27+02:00",
          "tree_id": "1a30a431236da6e348cdc3ccc383989f6f190b01",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/77c3618374a81a36ce90babbe37534ec05a10d4e"
        },
        "date": 1791693849073,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.867,
            "range": "± 0.07",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.108,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.153,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.235,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.286,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.17,
            "range": "± 0.039",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.251,
            "range": "± 0.057",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.14,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.067,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.194,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.145,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.349,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.119,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.09,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.074,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.233,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.145,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.795,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.155,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.036,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.391,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.316,
            "range": "± 0.15",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.806,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.092,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.073,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.326,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.335,
            "range": "± 0.038",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.445,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "81fca7282dd54d371ec93da3e40a21e91c80ad3e",
          "message": "Functions that read `func_get_args()`",
          "timestamp": "2026-10-11T18:34:14+02:00",
          "tree_id": "ea5353e5577a27038f8fda644fa0ecab2e6bdb40",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/81fca7282dd54d371ec93da3e40a21e91c80ad3e"
        },
        "date": 1791737027510,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.981,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.074,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.108,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.181,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.22,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.955,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.86,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.089,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.039,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.035,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.041,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.771,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.113,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.254,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.043,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.088,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.051,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.041,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.165,
            "range": "± 0",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.875,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.674,
            "range": "± 0.116",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.877,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.765,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.292,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 13.564,
            "range": "± 0.3",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.593,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.052,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.05,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.199,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.255,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.35,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4421a984ec34f5fc58b60e393e5746b93c842f91",
          "message": "Lower memory use on large projects",
          "timestamp": "2026-10-11T18:39:35+02:00",
          "tree_id": "8f71eb2211ca47fc3d1cd02a09d5ea6c7cb3733f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4421a984ec34f5fc58b60e393e5746b93c842f91"
        },
        "date": 1791737395692,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.841,
            "range": "± 0.162",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.106,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.154,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.243,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.285,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.202,
            "range": "± 0.063",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.257,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.139,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.057,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.055,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.065,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.195,
            "range": "± 0.046",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.146,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.351,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.078,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.047,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.119,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.087,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.076,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.229,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.145,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.841,
            "range": "± 0.064",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.141,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.08,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.39,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.262,
            "range": "± 0.43",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.8,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.088,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.08,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.314,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.338,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.453,
            "range": "± 0.009",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f99ae545b9c644e7deb2241c570510282e8cb435",
          "message": "`@not-deprecated` on an inherited method",
          "timestamp": "2026-10-11T19:09:51+02:00",
          "tree_id": "c7d544a1b826a71f5ade7b6dfddf2d9fdec12f59",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f99ae545b9c644e7deb2241c570510282e8cb435"
        },
        "date": 1791739169837,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.868,
            "range": "± 0.456",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.048,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.113,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.148,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.236,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.288,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.165,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.237,
            "range": "± 0.047",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.141,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.054,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.07,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.131,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.148,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.35,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.08,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.122,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.087,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.072,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.229,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.139,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.768,
            "range": "± 0.043",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.136,
            "range": "± 0.05",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.068,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.402,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.313,
            "range": "± 0.193",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.814,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.087,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.077,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.331,
            "range": "± 0.033",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.336,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.436,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "04e340e187e609eb8675b83d9d723e9d817b68f0",
          "message": "Lower memory use on large projects",
          "timestamp": "2026-10-11T19:09:24+02:00",
          "tree_id": "597810b727e696a3489d7336f15c12f1d1025d10",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/04e340e187e609eb8675b83d9d723e9d817b68f0"
        },
        "date": 1791739169708,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.321,
            "range": "± 0.224",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.111,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.165,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.246,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.3,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.188,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.441,
            "range": "± 0.073",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.144,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.062,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.062,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.071,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.348,
            "range": "± 0.037",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.152,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.365,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.068,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.127,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.09,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.071,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.231,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.172,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.942,
            "range": "± 0.279",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.189,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.115,
            "range": "± 0.06",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.402,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 19.222,
            "range": "± 0.299",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.827,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.089,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.074,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.313,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.348,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.459,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "95377c454420c8dfd20c58cbb7268e42a42c920e",
          "message": "Memory left behind by edited files is now reclaimed",
          "timestamp": "2026-10-11T19:25:22+02:00",
          "tree_id": "67b497d64119d08c415847faba3e78a1667ff4f0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/95377c454420c8dfd20c58cbb7268e42a42c920e"
        },
        "date": 1791740060381,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.324,
            "range": "± 0.078",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.026,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.055,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.08,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.13,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.146,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.655,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.281,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.06,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.033,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.03,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.872,
            "range": "± 0.057",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.099,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.189,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.032,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.029,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.075,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.033,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.128,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.699,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.069,
            "range": "± 0.159",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.682,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.577,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.232,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 10.659,
            "range": "± 0.534",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.497,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.036,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.037,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.147,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.191,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.261,
            "range": "± 0.009",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e7b1c2ccd4aa15bcc1514fcfc22b502a930aa696",
          "message": "Fix some templating issues",
          "timestamp": "2026-10-11T20:01:35+02:00",
          "tree_id": "e8906c73c53c6670e1333833a20eb2c7de483df3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e7b1c2ccd4aa15bcc1514fcfc22b502a930aa696"
        },
        "date": 1791742283348,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.812,
            "range": "± 0.104",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.044,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.113,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.156,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.243,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.287,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.163,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.256,
            "range": "± 0.063",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.137,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.058,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.077,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.379,
            "range": "± 0.035",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.144,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.337,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.047,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.121,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.09,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.074,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.224,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.124,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.708,
            "range": "± 0.029",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.14,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.02,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.395,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.605,
            "range": "± 0.111",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.806,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.093,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.087,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.314,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.349,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.438,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2753cc1285da59bdd9917cce1b8b34a35e82721b",
          "message": "Fix a few blade issues",
          "timestamp": "2026-10-11T21:14:43+02:00",
          "tree_id": "baeed2f8f1f74343c8f431628428c5075390dd10",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2753cc1285da59bdd9917cce1b8b34a35e82721b"
        },
        "date": 1791746639245,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.959,
            "range": "± 0.065",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.075,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.118,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.186,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.228,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.973,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.891,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.091,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.042,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.038,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.043,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.666,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.124,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.264,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.045,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.092,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.053,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.044,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.162,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.868,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.657,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.874,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.742,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.289,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 13.506,
            "range": "± 0.058",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.598,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.055,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.052,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.202,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.262,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.348,
            "range": "± 0.003",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2eeb7a815706fb47b35b196ae3870690b5552d3a",
          "message": "Fix a couple of return type issues from standard lib functions",
          "timestamp": "2026-10-11T21:15:53+02:00",
          "tree_id": "18c263db65ddc9ec4d16cad399c42f5373ba2845",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2eeb7a815706fb47b35b196ae3870690b5552d3a"
        },
        "date": 1791746706571,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.607,
            "range": "± 0.218",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.027,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.055,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.08,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.134,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.154,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.689,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.343,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.06,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.034,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.03,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 3.002,
            "range": "± 0.054",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.1,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.193,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.03,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.079,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.128,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.711,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.495,
            "range": "± 0.625",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.722,
            "range": "± 0.044",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.609,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.239,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 12.025,
            "range": "± 0.643",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.525,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.037,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.038,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.151,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.205,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.265,
            "range": "± 0.019",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f1d3da4878ee6a3085f7f3f33d33e52e9a7158f8",
          "message": "`@component` no longer shares the surrounding template's variables",
          "timestamp": "2026-10-11T21:23:24+02:00",
          "tree_id": "903291a2e6ad8c0bb60b1a5ff3e75393ee8d5521",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f1d3da4878ee6a3085f7f3f33d33e52e9a7158f8"
        },
        "date": 1791747126915,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.45,
            "range": "± 0.215",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.026,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.054,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.079,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.125,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.142,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.617,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.231,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.058,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.032,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.029,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.029,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.89,
            "range": "± 0.22",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.096,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.183,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.031,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.027,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.07,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.028,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.121,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.658,
            "range": "± 0.049",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 3.829,
            "range": "± 0.112",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.655,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.562,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.222,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 12.025,
            "range": "± 1.306",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.516,
            "range": "± 0.044",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.037,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.038,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.15,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.193,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.244,
            "range": "± 0.011",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ca2485f722142dbe76fa745275c02dca9fd84af9",
          "message": "Fix a few laravel type issues",
          "timestamp": "2026-10-11T21:56:12+02:00",
          "tree_id": "62a29312c4d321d4b82a09d7117559816dc9dc7d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ca2485f722142dbe76fa745275c02dca9fd84af9"
        },
        "date": 1791749143755,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.02,
            "range": "± 0.07",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.071,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.11,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.187,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.224,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.975,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.944,
            "range": "± 0.035",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.092,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.04,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.036,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.041,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.671,
            "range": "± 0.103",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.131,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.265,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.042,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.032,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.089,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.041,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.162,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.866,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.72,
            "range": "± 0.108",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.87,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.757,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.289,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 13.502,
            "range": "± 0.226",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.596,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.052,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.05,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.197,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.258,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.347,
            "range": "± 0.012",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d6d24cf86beea5cf675d056afe7695b858823f18",
          "message": "Fix narrowing issues",
          "timestamp": "2026-10-11T21:57:06+02:00",
          "tree_id": "fcc1481c661e8f23ed081dc042decb59f47d7747",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d6d24cf86beea5cf675d056afe7695b858823f18"
        },
        "date": 1791749211285,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.005,
            "range": "± 0.087",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.045,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.118,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.162,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.227,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.296,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.163,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.241,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.147,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.058,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.054,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.074,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.15,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.152,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.339,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.048,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.131,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.085,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.068,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.229,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.136,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.801,
            "range": "± 0.063",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.142,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.016,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.395,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.349,
            "range": "± 0.153",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.82,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.092,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.087,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.311,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.339,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.442,
            "range": "± 0.009",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3675f4757ffb1c7a84bba64c08cb4b7094a2cb83",
          "message": "PHPUnit's `assertSame()`, `assertCount()` and `assertArrayHasKey()`\nnarrow",
          "timestamp": "2026-10-11T22:14:35+02:00",
          "tree_id": "e2c53cc80409747ffb9f28996925c492816a49c1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3675f4757ffb1c7a84bba64c08cb4b7094a2cb83"
        },
        "date": 1791750327184,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.382,
            "range": "± 0.116",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.046,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.105,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.155,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.244,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.296,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.199,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.323,
            "range": "± 0.06",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.142,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.06,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.056,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.067,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.316,
            "range": "± 0.041",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.15,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.354,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.077,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.119,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.095,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.078,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.231,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.163,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.515,
            "range": "± 0.241",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.172,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.077,
            "range": "± 0.055",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.402,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.934,
            "range": "± 0.251",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.816,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.085,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.082,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.317,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.35,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.466,
            "range": "± 0.011",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c6c6cf9abdba7747058bd998246e866e1c2c6661",
          "message": "Fix a handful of array issues",
          "timestamp": "2026-10-11T22:28:11+02:00",
          "tree_id": "3ec48903d77a806ba4a9526ec5188dda901ead4a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c6c6cf9abdba7747058bd998246e866e1c2c6661"
        },
        "date": 1791750997747,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.332,
            "range": "± 0.132",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.024,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.052,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.075,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.125,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.14,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.624,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.211,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.057,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.031,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.029,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.028,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.736,
            "range": "± 0.042",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.092,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.178,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.03,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.027,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.07,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.028,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.117,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.648,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 3.713,
            "range": "± 0.079",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.654,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.51,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.222,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 9.477,
            "range": "± 0.075",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.468,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.034,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.035,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.138,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.184,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.238,
            "range": "± 0.015",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "nguyentranchung52th@gmail.com",
            "name": "Nguyễn Trần Chung",
            "username": "nguyentranchung"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "5035f035d53f216d5ff1b286a6fed8c9fa90d833",
          "message": "chore: update Mago dependencies (#443)",
          "timestamp": "2026-10-11T23:44:57+02:00",
          "tree_id": "2f3f6bd2d55898bee8af59e53d15196103e82418",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5035f035d53f216d5ff1b286a6fed8c9fa90d833"
        },
        "date": 1791755741991,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.001,
            "range": "± 0.123",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.045,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.111,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.156,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.237,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.288,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.166,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.234,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.138,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.062,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.058,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.076,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.345,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.141,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.321,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.076,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.116,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.094,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.075,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.226,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.128,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.795,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.144,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.052,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.404,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 17.664,
            "range": "± 0.286",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.817,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.087,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.081,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.337,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.333,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.443,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1c088a35e79269ec00d005f079c1a9a529ffb8ad",
          "message": "Conflicting trait properties",
          "timestamp": "2026-10-11T23:49:20+02:00",
          "tree_id": "04b43a361a495c3f5f63099bf35257b915d8cc2c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1c088a35e79269ec00d005f079c1a9a529ffb8ad"
        },
        "date": 1791755965086,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.243,
            "range": "± 0.43",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.045,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.116,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.154,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.244,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.291,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.192,
            "range": "± 0.043",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.272,
            "range": "± 0.056",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.149,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.059,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.057,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.07,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.218,
            "range": "± 0.04",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.155,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.354,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.076,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.048,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.129,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.088,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.08,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.231,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.146,
            "range": "± 0.042",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.949,
            "range": "± 0.346",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.164,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.09,
            "range": "± 0.048",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.395,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 19.016,
            "range": "± 0.539",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.826,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.086,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.084,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.361,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.355,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.44,
            "range": "± 0.011",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "be64c8038147fba3f8533c29d88e1b44797e0ee5",
          "message": "Destructuring offsets an array cannot have",
          "timestamp": "2026-10-11T23:55:29+02:00",
          "tree_id": "2d357f5c3316e684e02f707d9d01a4a8ddd4a04f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/be64c8038147fba3f8533c29d88e1b44797e0ee5"
        },
        "date": 1791756298639,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.983,
            "range": "± 0.053",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.029,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.074,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.114,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.181,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.215,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.95,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.85,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.08,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.039,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.035,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.041,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.663,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.113,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.265,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.043,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.089,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.052,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.04,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.161,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.869,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.628,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.876,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.77,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.319,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 13.885,
            "range": "± 0.051",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.614,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.053,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.046,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.194,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.253,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.344,
            "range": "± 0.003",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "77f3e25ee38793e6ba1ae26eb6de42113f28f0be",
          "message": "test(laravel): audit custom builders and relation callbacks",
          "timestamp": "2026-10-11T23:56:29+02:00",
          "tree_id": "4ab0fc189eed786f12535375b3ccc47b2c8d1c4b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/77f3e25ee38793e6ba1ae26eb6de42113f28f0be"
        },
        "date": 1791756381966,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.002,
            "range": "± 0.056",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.05,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.115,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.166,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.231,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.288,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.169,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.252,
            "range": "± 0.055",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.143,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.058,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.054,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.079,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.246,
            "range": "± 0.041",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.151,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.342,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.077,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.052,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.125,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.093,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.077,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.229,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.133,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.724,
            "range": "± 0.039",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.148,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.07,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.423,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.307,
            "range": "± 0.117",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.828,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.096,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.088,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.327,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.343,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.432,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1b823bd3a4b84f49c13d54efbe1bc8fbe3251451",
          "message": "Fix benchmark logging",
          "timestamp": "2026-10-12T05:10:00+02:00",
          "tree_id": "af406afeea28c184ac34229ceb96a0ed8eac13b4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1b823bd3a4b84f49c13d54efbe1bc8fbe3251451"
        },
        "date": 1791775253933,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.547,
            "range": "± 0.057",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.111,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.152,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.241,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.296,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.18,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.24,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.142,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.058,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.056,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.071,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.255,
            "range": "± 0.209",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.146,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.35,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.077,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.125,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.091,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.071,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.226,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.132,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.763,
            "range": "± 0.048",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.14,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.081,
            "range": "± 0.045",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.42,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.334,
            "range": "± 0.655",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.844,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.088,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.085,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.344,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.085,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.149,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.074,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.35,
            "range": "± 0.042",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.433,
            "range": "± 0.009",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "jwurtz@jolicode.com",
            "name": "Joel Wurtz",
            "username": "joelwurtz"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "78dcb15431712048f4ae1d9e61b8af3da5efd79e",
          "message": "[config] add the possibility to include specific files / directories for scanning\n\nCo-authored-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-10-12T06:15:44+02:00",
          "tree_id": "6f8f3500ccdbff2e326d7f2a17ff07a5af9f1a2c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/78dcb15431712048f4ae1d9e61b8af3da5efd79e"
        },
        "date": 1791779171488,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.568,
            "range": "± 0.354",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.075,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.112,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.183,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.224,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.976,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.916,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.093,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.04,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.035,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.042,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.67,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.121,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.265,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.043,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.032,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.09,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.053,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.04,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.162,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.867,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.876,
            "range": "± 0.178",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.872,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.799,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.316,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 14.904,
            "range": "± 0.278",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.627,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.053,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.047,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.195,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.05,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.106,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.052,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.264,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.34,
            "range": "± 0.008",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c3a4be436ee1f7a59dbfe5bd5ba5ac7abac56e58",
          "message": "Update Mago",
          "timestamp": "2026-10-12T06:27:30+02:00",
          "tree_id": "e54f7aefa82cd37005aff593fb2fd55984eb3673",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c3a4be436ee1f7a59dbfe5bd5ba5ac7abac56e58"
        },
        "date": 1791779932527,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.554,
            "range": "± 0.114",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.046,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.117,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.165,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.247,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.3,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.185,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.313,
            "range": "± 0.051",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.144,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.06,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.059,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.074,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.297,
            "range": "± 0.032",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.15,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.35,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.088,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.123,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.088,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.08,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.231,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.137,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.834,
            "range": "± 0.111",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.146,
            "range": "± 0.039",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.088,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.43,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.586,
            "range": "± 0.159",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.838,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.087,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.081,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.299,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.097,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.161,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.082,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.381,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.432,
            "range": "± 0.005",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3f8622e75c5841b261499bd74659963036be133a",
          "message": "Closure parameter hints keep their own class",
          "timestamp": "2026-10-12T06:38:38+02:00",
          "tree_id": "38ac63b40f40310e55c003a1fd4bdcb800067bb4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3f8622e75c5841b261499bd74659963036be133a"
        },
        "date": 1791780503525,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.733,
            "range": "± 0.275",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.027,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.056,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.079,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.135,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.147,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.702,
            "range": "± 0.067",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.331,
            "range": "± 0.058",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.062,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.034,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.031,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.03,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.95,
            "range": "± 0.122",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.093,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.189,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.029,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.073,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.125,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.678,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 3.841,
            "range": "± 0.09",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.679,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.589,
            "range": "± 0.04",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.245,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 10.728,
            "range": "± 0.563",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.507,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.036,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.036,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.145,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.038,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.028,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.067,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.042,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.193,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.289,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7996896f541f0b5aba2b13f726b7eb58e9f144c4",
          "message": "Fix a couple of array issues",
          "timestamp": "2026-10-12T06:50:57+02:00",
          "tree_id": "21411ba265bdd47e5bb1da263d443ac233afda56",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7996896f541f0b5aba2b13f726b7eb58e9f144c4"
        },
        "date": 1791781316762,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.519,
            "range": "± 0.112",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.037,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.098,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.142,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.236,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.287,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.251,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.439,
            "range": "± 0.017",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.119,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.049,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.044,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.052,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.8,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.141,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.33,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.055,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.04,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.115,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.067,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.054,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.21,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.134,
            "range": "± 0.035",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.055,
            "range": "± 0.056",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.151,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.007,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.409,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.359,
            "range": "± 0.156",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.807,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.068,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.064,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.251,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.069,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.043,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.138,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.071,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.337,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.436,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "441772b3e7d933b3c73f0dc869031ae59c41536e",
          "message": "Fix a few laravel issues",
          "timestamp": "2026-10-12T07:06:44+02:00",
          "tree_id": "8433c4434fe4f750227e94c134b5203e54b31f43",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/441772b3e7d933b3c73f0dc869031ae59c41536e"
        },
        "date": 1791782260720,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.441,
            "range": "± 0.117",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.047,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.104,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.16,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.243,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.291,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.175,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.265,
            "range": "± 0.064",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.142,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.06,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.073,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.31,
            "range": "± 0.029",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.149,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.358,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.117,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.092,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.072,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.229,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.133,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.719,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.137,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.071,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.434,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.407,
            "range": "± 0.28",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.839,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.081,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.08,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.303,
            "range": "± 0.029",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.084,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.052,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.15,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.08,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.335,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.443,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "251142cd29e7d51f1ec41d57e54b06da867a8ac0",
          "message": "Fix a couple of templating issues",
          "timestamp": "2026-10-12T07:26:04+02:00",
          "tree_id": "a10919ed74147b0b8c4086311cfe9374b2fac741",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/251142cd29e7d51f1ec41d57e54b06da867a8ac0"
        },
        "date": 1791783431665,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.554,
            "range": "± 0.279",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.046,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.118,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.155,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.243,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.296,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.167,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.287,
            "range": "± 0.076",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.143,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.058,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.053,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.072,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.217,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.149,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.34,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.052,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.121,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.093,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.077,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.231,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.168,
            "range": "± 0.022",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.703,
            "range": "± 0.322",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.167,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.114,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.443,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 20.218,
            "range": "± 0.456",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.877,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.094,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.082,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.345,
            "range": "± 0.033",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.091,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.159,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.082,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.342,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.457,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "pierre@janalis.com",
            "name": "phcorp",
            "username": "phcorp"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "5d3fe03aff52cb4b715b7a2ee7ce0f38756759cf",
          "message": "Narrow ternary branches inside the object of a property access",
          "timestamp": "2026-10-12T09:49:37+02:00",
          "tree_id": "b382f8756e006bd1c87f18cf47335d4998f4398b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5d3fe03aff52cb4b715b7a2ee7ce0f38756759cf"
        },
        "date": 1791792045828,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 5.227,
            "range": "± 0.194",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.04,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.098,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.144,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.241,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.277,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.176,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.389,
            "range": "± 0.052",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.112,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.052,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.048,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.053,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.545,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.18,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.342,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.055,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.043,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.123,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.065,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.053,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.214,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.177,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 6.858,
            "range": "± 0.129",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.196,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.124,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.419,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 19.343,
            "range": "± 0.248",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.953,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.069,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.066,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.266,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.066,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.043,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.13,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.067,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.242,
            "range": "± 0.019",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.346,
            "range": "± 0.007",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8fb41973be548e5c0cb559e52010268cd8e4fb81",
          "message": "Fix high CPU load",
          "timestamp": "2026-10-13T01:58:05+02:00",
          "tree_id": "49a81a3af0865c68e950e55115dff042334e5ae9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8fb41973be548e5c0cb559e52010268cd8e4fb81"
        },
        "date": 1791850077164,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.647,
            "range": "± 0.062",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.025,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.052,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.075,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.126,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.141,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.629,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.239,
            "range": "± 0.136",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.058,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.03,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.029,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.741,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.097,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.176,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.03,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.027,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.068,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.028,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.118,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.652,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 3.675,
            "range": "± 0.063",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.658,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.578,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.239,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 10.24,
            "range": "± 0.57",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.505,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.035,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.035,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.142,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.036,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.027,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.065,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.04,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.181,
            "range": "± 0.054",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.244,
            "range": "± 0.003",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "1c33a8ad929fca07d9005b419e46b28370ff38a3",
          "message": "fix(types): preserve callable array return types",
          "timestamp": "2026-10-13T04:35:36+02:00",
          "tree_id": "c18b1ed0f61daa4aff1e9c6f571448d815182e9e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1c33a8ad929fca07d9005b419e46b28370ff38a3"
        },
        "date": 1791859608406,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.851,
            "range": "± 0.335",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.048,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.111,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.159,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.246,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.293,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.159,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.294,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.14,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.057,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.059,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.074,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.328,
            "range": "± 0.027",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.15,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.333,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.072,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.055,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.124,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.084,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.082,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.225,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.12,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.75,
            "range": "± 0.087",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.128,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.071,
            "range": "± 0.028",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.435,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.655,
            "range": "± 0.228",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.842,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.086,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.08,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.321,
            "range": "± 0.031",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.085,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.049,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.154,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.079,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.345,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.448,
            "range": "± 0.006",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "pierre@janalis.com",
            "name": "phcorp",
            "username": "phcorp"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "03139902968b9e0dabcc47a3f5dee7fc3c51951e",
          "message": "Narrow closures created in ternary, && and match branches (#477)",
          "timestamp": "2026-10-13T05:14:39+02:00",
          "tree_id": "56b0323c511e1b08162e4573d6785bb29e6b39fb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/03139902968b9e0dabcc47a3f5dee7fc3c51951e"
        },
        "date": 1791861954790,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.537,
            "range": "± 0.182",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.047,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.104,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.159,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.236,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.295,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.153,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.25,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.14,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.06,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.053,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.066,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.205,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.148,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.357,
            "range": "± 0.023",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.086,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.123,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.101,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.07,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.23,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.13,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.738,
            "range": "± 0.046",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.146,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.071,
            "range": "± 0.048",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.429,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 18.798,
            "range": "± 0.394",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.839,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.089,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.086,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.33,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.091,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.052,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.155,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.084,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.339,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.446,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "pierre@janalis.com",
            "name": "phcorp",
            "username": "phcorp"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "30b1ba0e0b33abc104cbb175882630b1e47c1ed6",
          "message": "Check an invoked property against its value, not a same-named method",
          "timestamp": "2026-10-13T05:50:38+02:00",
          "tree_id": "afbdcb018ff82ada341f50df63dd49000ecf08cb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/30b1ba0e0b33abc104cbb175882630b1e47c1ed6"
        },
        "date": 1791864029456,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.761,
            "range": "± 0.103",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.026,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.052,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.076,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.127,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.147,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.645,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.251,
            "range": "± 0.036",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.059,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.03,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.963,
            "range": "± 0.042",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.107,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.19,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.032,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.029,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.073,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.034,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.127,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.72,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.274,
            "range": "± 0.166",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.741,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.652,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.271,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 11.952,
            "range": "± 0.2",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.579,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.04,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.041,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.161,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.04,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.031,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.076,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.046,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.214,
            "range": "± 0.029",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.278,
            "range": "± 0.007",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a2fe68ef852e1b7c5dbd4f4c29273d0c1c9c930c",
          "message": "Global classes next to an import of the same short name",
          "timestamp": "2026-10-13T19:17:13+02:00",
          "tree_id": "b333e80fbec2bee38995cab96bb2b188b80eb92e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a2fe68ef852e1b7c5dbd4f4c29273d0c1c9c930c"
        },
        "date": 1791912512203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 3.609,
            "range": "± 0.058",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.03,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.076,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.112,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.183,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.225,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.977,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.944,
            "range": "± 0.049",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.091,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.04,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.037,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.043,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 4.625,
            "range": "± 0.127",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.124,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.264,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.045,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.091,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.054,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.043,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.167,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.881,
            "range": "± 0.013",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.67,
            "range": "± 0.111",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.886,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.788,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.31,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 9.312,
            "range": "± 0.057",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.628,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.044,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.051,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.198,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.066,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.034,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.109,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.055,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.258,
            "range": "± 0.02",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.342,
            "range": "± 0.018",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b24b0af37f960387a3106b359dc259333a32453c",
          "message": "Classes inside a phar no longer go missing on a busy machine",
          "timestamp": "2026-10-13T21:06:55+02:00",
          "tree_id": "73c053e9c4c5591bd00a2e717635461d0f3c3f9c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b24b0af37f960387a3106b359dc259333a32453c"
        },
        "date": 1791919018759,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 2.79,
            "range": "± 0.324",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.026,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.056,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.081,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.136,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.152,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 0.679,
            "range": "± 0.029",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 1.329,
            "range": "± 0.222",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.061,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.033,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.03,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.032,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 2.887,
            "range": "± 0.155",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.105,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.189,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.033,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.029,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.074,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.033,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.029,
            "range": "± 0.001",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.125,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 0.692,
            "range": "± 0.046",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 4.195,
            "range": "± 0.174",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 0.687,
            "range": "± 0.06",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 0.622,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.25,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 7.978,
            "range": "± 0.56",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.523,
            "range": "± 0.034",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.033,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.038,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.162,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.043,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.03,
            "range": "± 0.003",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.075,
            "range": "± 0.015",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.045,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.222,
            "range": "± 0.042",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.304,
            "range": "± 0.027",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8f3e4f0622612a5f2e16ac98dd67c32697bed57b",
          "message": "`key-of` and `value-of` read in the function's own file",
          "timestamp": "2026-10-13T21:27:56+02:00",
          "tree_id": "80c8b5b55be39db642b866450872f600d291c1bf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8f3e4f0622612a5f2e16ac98dd67c32697bed57b"
        },
        "date": 1791920326559,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.66,
            "range": "± 0.216",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.046,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.1,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.155,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.225,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.293,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.169,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.393,
            "range": "± 0.078",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.131,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.056,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.051,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.063,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.346,
            "range": "± 0.039",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.148,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.332,
            "range": "± 0.018",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.076,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.049,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.123,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.08,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.066,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.23,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.162,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.889,
            "range": "± 0.016",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.165,
            "range": "± 0.024",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.078,
            "range": "± 0.01",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.418,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 11.934,
            "range": "± 0.085",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.847,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.065,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.076,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.292,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.081,
            "range": "± 0.012",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.05,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.144,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.074,
            "range": "± 0.004",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.333,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.417,
            "range": "± 0.004",
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ae3c62a04948410699a6a6e9981861b68640e950",
          "message": "A closure capturing a variable by reference sees later reassignments",
          "timestamp": "2026-10-13T21:28:20+02:00",
          "tree_id": "c9f50286183a03536053ac2ae19bd490a01c84e3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ae3c62a04948410699a6a6e9981861b68640e950"
        },
        "date": 1791920367659,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cold_start_completion",
            "value": 4.591,
            "range": "± 0.144",
            "unit": "ms"
          },
          {
            "name": "completion_simple_class",
            "value": 0.044,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_5",
            "value": 0.116,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_10",
            "value": 0.159,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_inheritance_depth/depth_20",
            "value": 0.246,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/100_classes",
            "value": 0.293,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/500_classes",
            "value": 1.172,
            "range": "± 0.026",
            "unit": "ms"
          },
          {
            "name": "completion_classmap_size/1000_classes",
            "value": 2.288,
            "range": "± 0.051",
            "unit": "ms"
          },
          {
            "name": "completion_generics_and_mixins",
            "value": 0.144,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "completion_with_narrowing",
            "value": 0.061,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_5_method_chain",
            "value": 0.054,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "completion_cross_file_type_hint",
            "value": 0.067,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "completion_carbon_class",
            "value": 5.303,
            "range": "± 0.047",
            "unit": "ms"
          },
          {
            "name": "completion_yii_deep_hierarchy",
            "value": 0.157,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "completion_large_file",
            "value": 0.36,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "completion_short_file",
            "value": 0.075,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "variable_completion/short",
            "value": 0.05,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "variable_completion/long",
            "value": 0.122,
            "range": "± 0.006",
            "unit": "ms"
          },
          {
            "name": "hover_method_call",
            "value": 0.099,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "goto_definition_method",
            "value": 0.069,
            "range": "± 0.008",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/100_lines",
            "value": 0.235,
            "range": "± 0.002",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/500_lines",
            "value": 1.171,
            "range": "± 0.025",
            "unit": "ms"
          },
          {
            "name": "update_ast_parse_time/2000_lines",
            "value": 5.959,
            "range": "± 0.054",
            "unit": "ms"
          },
          {
            "name": "reparse_500_line_file",
            "value": 1.175,
            "range": "± 0.014",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_generic_objects",
            "value": 1.089,
            "range": "± 0.021",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_new_objects",
            "value": 0.422,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/lots_of_missing_methods",
            "value": 12.084,
            "range": "± 0.291",
            "unit": "ms"
          },
          {
            "name": "diagnostics/fixture/method_chain",
            "value": 0.836,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "custom_builder_completion/custom_builder_deep_inheritance",
            "value": 0.075,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_where_prefix",
            "value": 0.085,
            "range": "± 0.007",
            "unit": "ms"
          },
          {
            "name": "laravel_completion/model_typing_sequence",
            "value": 0.345,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/1",
            "value": 0.09,
            "range": "± 0.011",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/1",
            "value": 0.056,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cold_confirmation/128",
            "value": 0.163,
            "range": "± 0.009",
            "unit": "ms"
          },
          {
            "name": "laravel_morph_column_completion/cached_request/128",
            "value": 0.08,
            "range": "± 0.005",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/50",
            "value": 0.354,
            "range": "± 0.03",
            "unit": "ms"
          },
          {
            "name": "find_references/hierarchy_scan/150",
            "value": 0.44,
            "range": "± 0.008",
            "unit": "ms"
          }
        ]
      }
    ]
  }
}