## Türkçe

**CarlUI 1.3.4: Dock artık sağ tık menüsünden sonra büyük ikonla kalmıyor ve tarayıcıda gezerken açık takılı kalmıyor.**

**Düzeltmeler**
- **Sağ tıktan sonra büyük kalan ikon:** Dock'ta bir öğeye sağ tıklayıp menüden bir işlem seçince ikon üzerine gelme büyüklüğünde kalıyor, fare yeniden üstüne gelene kadar küçülmüyordu. Menü açıkken farenin çıktığı bildirimi kayboluyor, komut öğeleri yeniden kurunca da dock hangi ikonun büyük olduğunu unutuyordu. Menü (ve klasör yığını) kapanınca dock artık imlecin gerçek yerine bakıp ikonları yerine oturtuyor.
- **Dock'un açık takılı kalması:** Önizlemedeki bir pencere kartına tıklayınca önizleme kapanıyor ama "imleç önizlemenin üstünde" bilgisi açık kalıyordu; dock da bu yüzden bir daha otomatik gizlenmiyordu. Önizleme kapanınca bu bilgi artık sıfırlanıyor, gizleme de dock, önizleme ve pil panelinde imlecin gerçekten üstlerinde olup olmadığını denetliyor.
- **Yarıda kalan sürükleme:** Bir ikonu sürüklerken başka bir pencere öne gelirse ya da Alt+Tab yapılırsa ikon bırakıldığı yerde ya da basılı boyutta kalabiliyordu. Artık sürükleme iptal edilip ikonlar eski sıralarına dönüyor.

---

## English

**CarlUI 1.3.4: The dock no longer leaves an icon enlarged after a right-click menu, and no longer gets stuck open while you browse.**

**Fixes**
- **Icon stuck enlarged after right-click:** After right-clicking a dock item and choosing an action, the icon stayed at its hover size until the mouse came over it again. The mouse-leave notice was lost while the menu was open, and when a command rebuilt the items the dock forgot which icon was enlarged. When the menu (or a folder stack) closes, the dock now checks where the cursor really is and settles every icon.
- **Dock stuck open:** Clicking a window card in the preview closed the preview, but the "cursor is over the preview" flag stayed on, so the dock never auto-hid again. The flag is now cleared when the preview closes, and auto-hide checks whether the cursor is really over the dock, the preview or the battery panel.
- **Interrupted drag:** If another window came to the front or you pressed Alt+Tab while dragging an icon, the icon could stay where it was dropped or at its pressed size. The drag is now cancelled and the icons return to their previous order.
