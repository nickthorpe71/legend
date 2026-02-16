#!/usr/bin/env node

import { Command } from 'commander';
import { readdir, writeFile, rm, stat } from 'fs/promises';
import { join } from 'path';
import { loadScenario, loadConfig } from './scenario.js';
import { runScenario } from './runner.js';
import { runDevLoop } from './loop/orchestrator.js';

const program = new Command();

program
  .name('stickler')
  .description('Autonomous UI validator for agent-driven development')
  .version('1.0.0');

// Run command
program
  .command('run <scenario>')
  .description('Run a scenario')
  .action(async (scenarioName: string) => {
    try {
      console.log('🚀 Stickler - Autonomous UI Validator\n');

      // Load scenario and config
      const scenario = await loadScenario(scenarioName);
      const config = await loadConfig();

      // Run the scenario
      const result = await runScenario(scenario, config);

      // Exit with appropriate code
      process.exit(result.status === 'PASS' ? 0 : 1);
    } catch (error) {
      console.error('\n❌ Error:', error instanceof Error ? error.message : String(error));
      process.exit(1);
    }
  });

// List command
program
  .command('list')
  .description('List available scenarios')
  .action(async () => {
    try {
      const scenariosDir = join(process.cwd(), 'scenarios');
      const files = await readdir(scenariosDir);
      const scenarios = files.filter((f) => f.endsWith('.json')).map((f) => f.replace('.json', ''));

      console.log('\n📋 Available scenarios:\n');
      if (scenarios.length === 0) {
        console.log('   (no scenarios found)');
        console.log('\n   Create one with: stickler init <name>\n');
      } else {
        scenarios.forEach((s) => console.log(`   - ${s}`));
        console.log('\n   Run with: stickler run <name>\n');
      }
    } catch (error) {
      console.error('❌ Error listing scenarios:', error instanceof Error ? error.message : String(error));
      process.exit(1);
    }
  });

// Init command
program
  .command('init <name>')
  .description('Create a new scenario template')
  .option('-u, --url <url>', 'Start URL', 'http://localhost:5173')
  .option('-o, --objective <objective>', 'Scenario objective', 'Complete the workflow')
  .action(async (name: string, options: { url: string; objective: string }) => {
    try {
      const template = {
        name,
        start_url: options.url,
        objective: options.objective,
        success_signals: ['success', 'complete', 'done'],
        fail_signals: ['error', 'failed', 'invalid'],
        max_steps: 20,
        context: 'Additional context about this scenario (optional)',
      };

      const scenarioPath = join(process.cwd(), 'scenarios', `${name}.json`);
      await writeFile(scenarioPath, JSON.stringify(template, null, 2));

      console.log(`\n✅ Created scenario template: ${scenarioPath}`);
      console.log('\n📝 Edit the file to customize:');
      console.log(`   - objective: What you want to achieve`);
      console.log(`   - success_signals: Text/elements that indicate success`);
      console.log(`   - fail_signals: Text/elements that indicate failure`);
      console.log('\n🚀 Run with: stickler run ' + name + '\n');
    } catch (error) {
      console.error('❌ Error creating scenario:', error instanceof Error ? error.message : String(error));
      process.exit(1);
    }
  });

// Clean command
program
  .command('clean')
  .description('Clean up old test run directories')
  .option('-a, --all', 'Delete all run directories')
  .option('-h, --hours <hours>', 'Delete runs older than X hours', '24')
  .action(async (options: { all?: boolean; hours: string }) => {
    try {
      const runsDir = join(process.cwd(), 'runs');
      const files = await readdir(runsDir);

      if (files.length === 0) {
        console.log('\n📂 No run directories found\n');
        return;
      }

      const now = Date.now();
      const hoursInMs = parseFloat(options.hours) * 60 * 60 * 1000;

      let deletedCount = 0;
      let keptCount = 0;

      console.log(options.all
        ? '\n🧹 Deleting all run directories...\n'
        : `\n🧹 Deleting runs older than ${options.hours} hours...\n`);

      for (const file of files) {
        const dirPath = join(runsDir, file);

        if (options.all) {
          await rm(dirPath, { recursive: true, force: true });
          console.log(`   ❌ Deleted: ${file}`);
          deletedCount++;
        } else {
          const stats = await stat(dirPath);
          const ageInMs = now - stats.mtimeMs;

          if (ageInMs > hoursInMs) {
            await rm(dirPath, { recursive: true, force: true });
            const ageInHours = (ageInMs / (60 * 60 * 1000)).toFixed(1);
            console.log(`   ❌ Deleted: ${file} (${ageInHours}h old)`);
            deletedCount++;
          } else {
            keptCount++;
          }
        }
      }

      console.log(`\n✅ Cleanup complete:`);
      console.log(`   - Deleted: ${deletedCount} directories`);
      if (!options.all) {
        console.log(`   - Kept: ${keptCount} recent directories`);
      }
      console.log();
    } catch (error) {
      console.error('❌ Error cleaning runs:', error instanceof Error ? error.message : String(error));
      process.exit(1);
    }
  });

// Dev Loop command
program
  .command('dev-loop <prd-name>')
  .description('Run autonomous development loop from PRD')
  .option('-m, --max-iterations <number>', 'Maximum iterations', '5')
  .option('--max-same-error <number>', 'Stop if same error repeats N times', '3')
  .option('-v, --verbose', 'Verbose logging', false)
  .option('-w, --workspace <path>', 'Workspace root directory', '../')
  .action(async (prdName: string, options: {
    maxIterations: string;
    maxSameError: string;
    verbose: boolean;
    workspace: string;
  }) => {
    try {
      const prdPath = join(process.cwd(), 'prds', `${prdName}.md`);

      const config = {
        prdPath,
        maxIterations: parseInt(options.maxIterations, 10),
        maxSameError: parseInt(options.maxSameError, 10),
        verbose: options.verbose,
        workspaceRoot: join(process.cwd(), options.workspace),
      };

      const report = await runDevLoop(config);

      // Exit with appropriate code
      process.exit(report.status === 'success' ? 0 : 1);
    } catch (error) {
      console.error('\n❌ Error:', error instanceof Error ? error.message : String(error));
      process.exit(1);
    }
  });

program.parse();
