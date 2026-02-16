import { readFile } from 'fs/promises';
import { join } from 'path';
import { Scenario, ScenarioSchema } from './ui/types.js';

export async function loadScenario(scenarioName: string): Promise<Scenario> {
  const scenarioPath = join(process.cwd(), 'scenarios', `${scenarioName}.json`);

  try {
    const content = await readFile(scenarioPath, 'utf-8');
    const data = JSON.parse(content);
    return ScenarioSchema.parse(data);
  } catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'ENOENT') {
      throw new Error(`Scenario not found: ${scenarioName}`);
    }
    throw error;
  }
}

export async function loadConfig() {
  const configPath = join(process.cwd(), 'stickler.config.json');
  const content = await readFile(configPath, 'utf-8');
  return JSON.parse(content);
}
