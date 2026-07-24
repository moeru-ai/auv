import { definePlugin } from "@alint-js/plugin";

import { noModNamesChecksInTestsRule } from "./rules/no-mod-names-checks-in-tests";
import { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
import { noSourceFilesCompareInTestsRule } from "./rules/no-source-files-compare-in-tests";
import { privateSchemaToolkitRule } from "./rules/no-private-schema-toolkit";
import { establishedFoundationRule } from "./rules/prefer-established-foundation";
import { platformScopedAppIntegrationRule } from "./rules/require-platform-scoped-app-integration";

export { noModNamesChecksInTestsRule } from "./rules/no-mod-names-checks-in-tests";
export { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
export { noSourceFilesCompareInTestsRule } from "./rules/no-source-files-compare-in-tests";
export { privateSchemaToolkitRule } from "./rules/no-private-schema-toolkit";
export { establishedFoundationRule } from "./rules/prefer-established-foundation";
export { platformScopedAppIntegrationRule } from "./rules/require-platform-scoped-app-integration";

export default definePlugin({
  rules: {
    "no-mod-names-checks-in-tests": noModNamesChecksInTestsRule,
    "no-vacant-control-boundary": vacantControlBoundaryRule,
    "no-source-files-compare-in-tests": noSourceFilesCompareInTestsRule,
    "no-private-schema-toolkit": privateSchemaToolkitRule,
    "prefer-established-foundation": establishedFoundationRule,
    "require-platform-scoped-app-integration": platformScopedAppIntegrationRule,
  },
});
