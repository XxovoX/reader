<template>
  <div class="hover-resize-handles" aria-hidden="true">
    <div v-for="edge in edges" :key="edge" class="hover-resize-edge" :class="'resize-' + edge"
      @pointerdown.stop.prevent="start($event, edge)" @click.stop @contextmenu.stop.prevent />
  </div>
</template>

<script setup lang="ts">
const emit = defineEmits<{ resize: [edge: string] }>()
const edges = ['North', 'South', 'East', 'West', 'NorthEast', 'NorthWest', 'SouthEast', 'SouthWest']
function start(event: PointerEvent, edge: string) {
  if (event.button === 0) emit('resize', edge)
}
</script>

<style scoped>
.hover-resize-handles { position:absolute; inset:0; pointer-events:none; z-index:2100; }
.hover-resize-edge { position:absolute; pointer-events:auto; touch-action:none; }
.hover-resize-edge::after { content:""; position:absolute; inset:0; border-radius:2px; opacity:0; background:rgba(255,255,255,.28); }
.hover-resize-edge:hover::after { opacity:1; }
.resize-North, .resize-South { left:12px; right:12px; height:7px; cursor:ns-resize; }
.resize-North { top:0; } .resize-South { bottom:0; }
.resize-East, .resize-West { top:12px; bottom:12px; width:7px; cursor:ew-resize; }
.resize-East { right:0; } .resize-West { left:0; }
.resize-NorthEast, .resize-NorthWest, .resize-SouthEast, .resize-SouthWest { width:12px; height:12px; }
.resize-NorthEast { top:0; right:0; cursor:nesw-resize; }
.resize-NorthWest { top:0; left:0; cursor:nwse-resize; }
.resize-SouthEast { bottom:0; right:0; cursor:nwse-resize; }
.resize-SouthWest { bottom:0; left:0; cursor:nesw-resize; }
</style>
