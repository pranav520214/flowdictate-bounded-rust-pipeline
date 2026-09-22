import fs from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const root='C:/Users/RYZEN/Desktop/automation for';
const skill='C:/Users/RYZEN/.codex/plugins/cache/openai-primary-runtime/presentations/26.903.11726/skills/presentations';
const {finalizePresentation}=await import(pathToFileURL(skill+'/container_tools/artifact_tool_utils.mjs'));
const result=await finalizePresentation({workspaceDir:root,candidatePath:root+'/.presentation-build/animated.pptx',finalPath:root+'/output/final/Privantrix_Avanta_APSJRC.pptx',pythonExecutable:'C:/Users/RYZEN/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe',integrityValidatorPath:skill+'/container_tools/inspect_presentation_package_integrity.py',layoutValidatorPath:skill+'/container_tools/inspect_presentation_layout_geometry.py',layoutArgs:['--expected-slide-size-emu','12192000,6858000','--validate-bullet-geometry','--validate-heading-fit','--require-native-table-slide','10','--require-native-table-slide','11'],explicitTotalSlideCount:12,requiredNativeTableOwnerSlides:[10,11],requiredNativeChartOwnerSlides:[],fontPolicy:{basis:'design',families:['Arial']},verifyArtifactToolImport:true,receiptPath:root+'/.presentation-build/final-validation-v2.json'});
console.log(JSON.stringify(result));
