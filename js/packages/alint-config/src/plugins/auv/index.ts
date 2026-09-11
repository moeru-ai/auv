import { definePlugin } from '@alint-js/plugin'

import { noModNamesChecksInTestsRule } from './rules/no-mod-names-checks-in-tests'
import { privateSchemaToolkitRule } from './rules/no-private-schema-toolkit'
import { noSourceFilesCompareInTestsRule } from './rules/no-source-files-compare-in-tests'
import { unearnedFunctionBoundaryRule } from './rules/no-unearned-function-boundary'
import { vacantControlBoundaryRule } from './rules/no-vacant-control-boundary'
import { establishedFoundationRule } from './rules/prefer-established-foundation'
import { platformScopedAppIntegrationRule } from './rules/require-platform-scoped-app-integration'
import { sideBySideUnitTestsRule } from './rules/require-side-by-side-unit-tests'
import { nonRuntimeUnitTestsRule } from './rules/restrict-non-runtime-unit-tests'

export { noModNamesChecksInTestsRule } from './rules/no-mod-names-checks-in-tests'
export { privateSchemaToolkitRule } from './rules/no-private-schema-toolkit'
export { noSourceFilesCompareInTestsRule } from './rules/no-source-files-compare-in-tests'
export { unearnedFunctionBoundaryRule } from './rules/no-unearned-function-boundary'
export { vacantControlBoundaryRule } from './rules/no-vacant-control-boundary'
export { establishedFoundationRule } from './rules/prefer-established-foundation'
export { platformScopedAppIntegrationRule } from './rules/require-platform-scoped-app-integration'
export { sideBySideUnitTestsRule } from './rules/require-side-by-side-unit-tests'
export { nonRuntimeUnitTestsRule } from './rules/restrict-non-runtime-unit-tests'

export default definePlugin({
  rules: {
    'no-mod-names-checks-in-tests': noModNamesChecksInTestsRule,
    'no-private-schema-toolkit': privateSchemaToolkitRule,
    'no-source-files-compare-in-tests': noSourceFilesCompareInTestsRule,
    'no-unearned-function-boundary': unearnedFunctionBoundaryRule,
    'no-vacant-control-boundary': vacantControlBoundaryRule,
    'prefer-established-foundation': establishedFoundationRule,
    'require-platform-scoped-app-integration': platformScopedAppIntegrationRule,
    'require-side-by-side-unit-tests': sideBySideUnitTestsRule,
    'restrict-non-runtime-unit-tests': nonRuntimeUnitTestsRule,
  },
})
