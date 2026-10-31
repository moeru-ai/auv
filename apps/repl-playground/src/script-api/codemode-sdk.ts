import * as sdk from '@auv-js/sdk'

import { codeModeExports } from './codemode-exports'

/** The SDK surface admitted into the agent VM. Help uses this same selection. */
export const codeModeSdk = Object.fromEntries(codeModeExports.map(name => [name, sdk[name]])) as Pick<typeof sdk, typeof codeModeExports[number]>
