/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.manager;

import info.embryosys.adt.request.AdtRequest;

/**
 * @author nrdufour
 *
 */
public interface AdtManager {

	public abstract void init();

	public abstract void processRequest(final AdtRequest request);

}