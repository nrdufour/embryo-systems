/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.core;

import java.util.UUID;

/**
 * @author nrdufour
 * 
 */
public class AdtFactory {

	public static Adt createNewAdt(final AdtType type, final String name) {
		Adt adt = new Adt();

		UUID uuid = UUID.randomUUID();
		AdtId id = new AdtId(type, uuid);

		adt.setId(id);
		adt.setName(name);

		return adt;
	}
}
