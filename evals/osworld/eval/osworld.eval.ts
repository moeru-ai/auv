import { describeTask, expect } from 'vieval'

import { loadCases } from '../src/cases'
import { runEpisode } from '../src/episode'

const cases = loadCases()

describeTask('OSWorld Kubernetes', ({ casesFromInputs }) => {
  casesFromInputs('OSWorld case', cases, async ({ matrix, metric, score, signal }) => {
    const item = matrix.inputs
    metric('benchmark.case.id', item.task.id)
    metric('osworld.case.id', item.id)
    metric('osworld.upstream.revision', item.upstream.revision)
    metric('auv.binary.sha256', item.auv.linuxSha256)
    metric('auv.macos_binary.sha256', item.auv.macosSha256)

    const result = await runEpisode(item, signal)
    metric('osworld.pod', result.podName)
    metric('osworld.evaluator.output', result.evaluatorOutput)
    metric('auv.run.ids', result.runIds)
    metric('auv.final_artifact.path', result.artifactPath)
    metric('auv.final_artifact.sha256', result.artifactSha256)
    score(result.score, 'exact')
    expect(result.score).toBe(1)
  }, {
    concurrency: 1,
    timeout: 20 * 60 * 1000,
  })
}, {
  description: 'Runs pinned OSWorld tasks in disposable KVM Pods and scores them with their official evaluators.',
})
