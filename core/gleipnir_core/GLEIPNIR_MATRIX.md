# GLEIPNIR CHECK MATRIX

**Legend:** 🔴 = Blocked | 🟠 = Error | 🟡 = Warning | ⚫ = Not applied

**Rows:** Check names (grouped by category)
**Columns:** File classifications

---

## V1 Classifications (legacy zone layout)

| Check                    | Script | Test | DataStr | UnsImp | UnsPur | ImpFn | PurFn | Outside |
|--------------------------|--------|------|---------|--------|--------|-------|-------|---------|
| **TYPE SAFETY**          |        |      |         |        |        |       |       |         |
| no_any_types             |   🟠   |  🟠  |   🟠    |  ⚫  |  ⚫  |  🟠   |  🟠   |   🟠    |
| no_any_type_aliases      |   🟠   |  ⚫  |   🟠    |  ⚫  |  ⚫  |  🟠   |  🟠   |   🟠    |
| no_object                |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_json_value            |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_bare_collections      |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_implicit_type_aliases |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| union_member_count       |   🟡   |  ⚫  |   🟡    |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| no_string_annotations    |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_cast                  |   🟠   |  ⚫  |   🟠    |  ⚫  |  ⚫  |  🟠   |  🟠   |   🟠    |
| no_callable_params       |   🔴   |  ⚫  |   🔴    |  ⚫  |  ⚫  |  🔴   |  🔴   |   🔴    |
| no_callable_type_aliases |   🔴   |  ⚫  |   🔴    |  ⚫  |  ⚫  |  🔴   |  🔴   |   🔴    |
| **IMPORTS**              |        |      |         |        |        |       |       |         |
| no_unsafe_imports        |   🔴   |  ⚫  |   🔴    |  ⚫  |  ⚫  |  🔴   |  🔴   |   🔴    |
| no_relative_imports      |   🔴   |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| impure_module_quarantine |  ⚫  |  ⚫  |  ⚫  |  ⚫  |   🟠   |  ⚫  |  🟠   |  ⚫  |
| no_type_checking_imports |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_parent_imports        |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_disallowed_stdlib     |  ⚫  |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_direct_file_io        |  ⚫  |  ⚫  |   🔴    |   🔴   |   🔴   |  🔴   |  🔴   |   🔴    |
| no_sys_path_mutation     |   🔴   |  🔴  |   🔴    |   🔴   |   🔴   |  🔴   |  🔴   |   🔴    |
| no_deferred_imports      |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| **PROHIBITED**           |        |      |         |        |        |       |       |         |
| no_bare_except           |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_broad_exceptions      |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_print                 |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_model_dump            |   🟠   |  ⚫  |   🟠    |  ⚫  |  ⚫  |  🟠   |  🟠   |   🟠    |
| no_overload              |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_future_annotations    |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| init_files_empty         |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_dunder_all            |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_suppression_comments  |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_nested_functions      |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_general_lambda        |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_default_factory       |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_partial               |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_re_sub                |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_recursion             |   🟠   |  🟠  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| **ARCHITECTURE**         |        |      |         |        |        |       |       |         |
| no_methods_in_classes    |   🟠   |  ⚫  |   🟠    |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| pydantic_only            |   🟠   |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| god_classes              |   🟠   |  ⚫  |  ⚫  |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| no_callable_protocol     |   🔴   |  ⚫  |  ⚫  |   🔴   |   🔴   |  🔴   |  🔴   |   🔴    |
| no_inline_dispatch       |   🔴   |  ⚫  |  ⚫  |   🔴   |   🔴   |  🔴   |  🔴   |   🔴    |
| no_reexport_shims        |  ⚫  |  ⚫  |  ⚫  |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| hardcoded_config         |  ⚫  |  ⚫  |   🟠    |   🟠   |   🟠   |  🟠   |  🟠   |   🟠    |
| classes_only_in_structures|  ⚫  |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| structures_no_functions  |  ⚫  |  ⚫  |   🟡    |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| structures_import_boundary|  ⚫  |  ⚫  |   🟠    |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| import_count             |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  🟡   |  🟡   |  ⚫  |
| max_functions_outside_zones    |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |   🟡    |
| **STYLE**                |        |      |         |        |        |       |       |         |
| function_length          |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| param_count              |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |  ⚫  |
| nesting_depth            |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| no_underscore_prefix     |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| no_none_returns          |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |  ⚫  |
| no_throwaway_assignment  |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| no_single_letter_names   |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| no_numbered_suffixes     |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| short_param_names        |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |
| short_local_names        |   🟡   |  ⚫  |  ⚫  |   🟡   |   🟡   |  🟡   |  🟡   |   🟡    |

---

## V2 Classifications (zone architecture)

| Check                    |  Str  |  Gen  | Model | Config |  Exm  | Pure  | PurDis |  Imp  | ImpDis | Trans | TrnDis |  Orch | OrchDis | Entry |
|--------------------------|-------|-------|-------|--------|-------|-------|--------|-------|--------|-------|--------|-------|---------|-------|
| **TYPE SAFETY**          |       |       |       |        |       |       |        |       |        |       |        |       |         |       |
| no_any_types             |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_any_type_aliases      |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_object                |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_json_value            |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_bare_collections      |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_implicit_type_aliases |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| union_member_count       |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| no_string_annotations    |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_cast                  |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_callable_params       |  🔴   |  🔴   |  🔴   |   🔴   |  🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴    |  🔴   |
| no_callable_type_aliases |  🔴   |  🔴   |  🔴   |   🔴   |  🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴    |  🔴   |
| **IMPORTS**              |       |       |       |        |       |       |        |       |        |       |        |       |         |       |
| no_type_checking_imports |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_disallowed_stdlib     |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_direct_file_io        |  🔴   |  🔴   |  🔴   |   🔴   |  🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴    |  🔴   |
| v2_import_boundaries     |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_before_validators     |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_sys_path_mutation     |  🔴   |  🔴   |  🔴   |   🔴   |  🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴    |  🔴   |
| no_deferred_imports      |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| impure_module_quarantine |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  🟠   |   🟠   |  ⚫  |  ⚫  |  🟠   |   🟠   |  ⚫  |  ⚫  |  ⚫  |
| **PROHIBITED**           |       |       |       |        |       |       |        |       |        |       |        |       |         |       |
| no_bare_except           |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_broad_exceptions      |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_print                 |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_model_dump            |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_overload              |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_future_annotations    |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| init_files_empty         |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_dunder_all            |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_suppression_comments  |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_nested_functions      |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_general_lambda        |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_default_factory       |  🟠   |  ⚫  |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_partial               |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_re_sub                |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_recursion             |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| **ARCHITECTURE**         |       |       |       |        |       |       |        |       |        |       |        |       |         |       |
| pydantic_only            |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| god_classes              |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_callable_protocol     |  🔴   |  🔴   |  🔴   |   🔴   |  🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴    |  🔴   |
| no_inline_dispatch       |  🔴   |  🔴   |  🔴   |   🔴   |  🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴    |  🔴   |
| hardcoded_config         |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| unknown_file_in_zone     |  🔴   |  🔴   |  🔴   |   🔴   |  🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴   |  🔴   |   🔴    |  🔴   |
| v2_cc_level              |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_methods_in_classes    |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| v2_structure_no_logic    |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| v2_structure_bases       |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| v2_structure_import_boundary  |  🟠   |  🟠   |  🟠   |   🟠   |  🟠   |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |
| v2_logic_no_constants    |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| no_reexport_shims        |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠   |  🟠   |   🟠    |  🟠   |
| v2_dispatch_only_tables  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |   🟠   |  ⚫  |   🟠   |  ⚫  |   🟠   |  ⚫  |   🟠    |  ⚫  |
| v2_classes_only_in_structure   |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  ⚫  |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| **STYLE**                |       |       |       |        |       |       |        |       |        |       |        |       |         |       |
| function_length          |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| param_count              |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  ⚫  |
| nesting_depth            |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| no_underscore_prefix     |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| no_none_returns          |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  ⚫  |
| no_throwaway_assignment  |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| no_single_letter_names   |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| no_numbered_suffixes     |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| short_param_names        |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
| short_local_names        |  🟡   |  🟡   |  🟡   |   🟡   |  🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡   |  🟡   |   🟡    |  🟡   |
