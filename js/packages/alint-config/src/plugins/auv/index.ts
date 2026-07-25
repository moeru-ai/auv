import { definePlugin } from "@alint-js/plugin";

import { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
import { privateSchemaToolkitRule } from "./rules/no-private-schema-toolkit";
import { establishedFoundationRule } from "./rules/prefer-established-foundation";
import { platformScopedAppIntegrationRule } from "./rules/require-platform-scoped-app-integration";

export { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
export { privateSchemaToolkitRule } from "./rules/no-private-schema-toolkit";
export { establishedFoundationRule } from "./rules/prefer-established-foundation";
export { platformScopedAppIntegrationRule } from "./rules/require-platform-scoped-app-integration";

export default definePlugin({
  rules: {
    "no-vacant-control-boundary": vacantControlBoundaryRule,
    "no-private-schema-toolkit": privateSchemaToolkitRule,
    "prefer-established-foundation": establishedFoundationRule,
    "require-platform-scoped-app-integration": platformScopedAppIntegrationRule,
  },
});
