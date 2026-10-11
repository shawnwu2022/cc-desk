const ts = require('typescript');

// 独立 Node 按 Vite 生产语义关闭 DEV；使用 AST，避免改写粘贴正文中的相同字面量。
function transpileProductionSource(source) {
  return ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    transformers: {
      before: [context => sourceFile => {
        const visit = node => {
          if (ts.isPropertyAccessExpression(node) && node.name.text === 'DEV'
            && ts.isPropertyAccessExpression(node.expression) && node.expression.name.text === 'env'
            && ts.isMetaProperty(node.expression.expression)
            && node.expression.expression.keywordToken === ts.SyntaxKind.ImportKeyword
            && node.expression.expression.name.text === 'meta') {
            return context.factory.createFalse();
          }
          return ts.visitEachChild(node, visit, context);
        };
        return ts.visitNode(sourceFile, visit);
      }],
    },
  }).outputText;
}

module.exports = { transpileProductionSource };
