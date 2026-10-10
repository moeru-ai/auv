import { defineConfig } from 'vieval'

export default defineConfig({
  concurrency: {
    workspace: 1,
  },
  models: [
    {
      aliases: [],
      id: 'local:auv-pr258',
      inferenceExecutor: 'local-auv',
      inferenceExecutorId: 'local:auv-pr258',
      model: 'auv-pr258',
    },
  ],
  projects: [
    {
      concurrency: {
        case: 1,
        project: 1,
        task: 1,
      },
      include: ['eval/*.eval.ts'],
      name: 'osworld-kubernetes',
      root: '.',
    },
  ],
})
