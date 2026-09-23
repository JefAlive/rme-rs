editor de mapa:
- exibir todos itens no mapa, não apenas o ground
- alguns itens quando stackam eles elevam um pouco, existe alguma propriedade pra isso
- animar os itens que são animáveis, é algo similar a um gif
- a forma que os andares estão alinhados está incorreta: um mesmo SQM um andar acima exibe em x-1 e y-1, e seu z é z-1

trabalhar os anti-aliasing da seguinte forma:
- off: nearest neighbour
- retro: sharp bilinear (single pass ou via UV)
- blurry: Super 2xSaI
- rounded edges: xBRZ
- crt blend: tirar essa opção

zoom out:
- usar um bom algoritmo de downscaling pra zoom out extremo (exemplo: em 25%)

shaders:
- colocar a opção no menu junto com anti-aliasing
- checkerboard dithering: (on/off), rodar um madapt antes do upscaling quando a imagem ainda está em 1x
- crt color: (on/off) aplicar fielmente a cor do fósforo P22 pra dar aquele efeito no RGB que o R desvia meio pro laranja, e o azul também tem aquele desvio característico, bem como o verde
- crt bloom: (on/off) aplicar um bloom que traga o feel de monitor CRT, porém sem scanlines. o bloom deve ser bom pra pixel art, e sem estourar nos brancos. o bloom deve ser aplicado depois do upscaling. ou seja, se o zoom tá em 200%, a extensão do bloom deve ser o dobro também, porque ele é aplicado no pós processamento

world light:
- quando está em 100%, fica a mesma cor de dia do tibia (pode usar o otclient como referência)
- quando está em 0%, fica noite. existe uma técnica usada por estúdios de cinema que pra deixar uma imagem de noite deve dar uma dessaturada e azulada no tom, pode se inspirar em algo assim, e esse slider do world light pode afetar alguns parâmetros do crt bloom pra calibrar o tanto que o vermelho, verde e azul espalham a luz

light:
- (on/off)
- quando ligado existe o world light, quando desligado os sprites ficam originais (sem luz)
- quando on, os objetos que possuem luz própria devem emitir a luz (se inspirar no esquema de luz do otclient)